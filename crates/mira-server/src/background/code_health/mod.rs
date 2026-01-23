// crates/mira-server/src/background/code_health/mod.rs
// Background worker for detecting code health issues using concrete signals

mod analysis;
mod cargo;
mod detection;

use crate::db::{
    get_indexed_projects_sync, get_scan_info_sync, is_time_older_than_sync,
    memory_key_exists_sync, mark_health_scanned_sync, clear_old_health_issues_sync,
    Database,
};
use crate::llm::DeepSeekClient;
use std::path::Path;
use std::sync::Arc;


/// Check code health for all indexed projects
pub async fn process_code_health(
    db: &Arc<Database>,
    deepseek: Option<&Arc<DeepSeekClient>>,
) -> Result<usize, String> {
    tracing::debug!("Code health: checking for projects needing scan");
    // Run on blocking thread to avoid blocking tokio
    let db_clone = db.clone();
    let projects = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        get_projects_needing_health_check(&conn)
    }).await.map_err(|e| format!("spawn_blocking panicked: {}", e))??;
    if !projects.is_empty() {
        tracing::info!("Code health: found {} projects needing scan", projects.len());
    }
    let mut processed = 0;

    for (project_id, project_path) in projects {
        if !Path::new(&project_path).exists() {
            continue;
        }

        match scan_project_health(db, deepseek, project_id, &project_path).await {
            Ok(count) => {
                tracing::info!(
                    "Found {} health issues for project {} ({})",
                    count,
                    project_id,
                    project_path
                );
                processed += count;
                // Run on blocking thread
                let db_clone = db.clone();
                tokio::task::spawn_blocking(move || {
                    let conn = db_clone.conn();
                    mark_health_scanned(&conn, project_id)
                }).await.map_err(|e| format!("spawn_blocking panicked: {}", e))??;
            }
            Err(e) => {
                tracing::warn!("Failed to scan health for {}: {}", project_path, e);
            }
        }
    }

    Ok(processed)
}

/// Get projects that need health scanning (same rate limiting as capabilities)
fn get_projects_needing_health_check(conn: &rusqlite::Connection) -> Result<Vec<(i64, String)>, String> {
    // Get all indexed projects
    let all_projects = get_indexed_projects_sync(conn).map_err(|e| e.to_string())?;

    let mut needing_scan = Vec::new();

    for (project_id, project_path) in all_projects {
        if needs_health_scan(conn, project_id)? {
            needing_scan.push((project_id, project_path));
            break; // One project per cycle
        }
    }

    Ok(needing_scan)
}

/// Check if project needs health scanning
/// Triggers when:
/// 1. Never scanned before
/// 2. Files changed (health_scan_needed flag is set)
/// 3. Fallback: > 1 day since last scan
fn needs_health_scan(conn: &rusqlite::Connection, project_id: i64) -> Result<bool, String> {
    // Check if the "needs scan" flag is set (triggered by file watcher)
    if memory_key_exists_sync(conn, project_id, "health_scan_needed") {
        return Ok(true);
    }

    // Check last scan time for fallback
    let scan_info = get_scan_info_sync(conn, project_id, "health_scan_time");

    match scan_info {
        None => Ok(true), // Never scanned
        Some((_, scan_time)) => {
            // Fallback: rescan if > 1 day old
            Ok(is_time_older_than_sync(conn, &scan_time, "-1 day"))
        }
    }
}

/// Mark project as health-scanned and clear the "needs scan" flag
fn mark_health_scanned(conn: &rusqlite::Connection, project_id: i64) -> Result<(), String> {
    mark_health_scanned_sync(conn, project_id).map_err(|e| e.to_string())
}

/// Mark project as needing a health scan (called by file watcher)
pub fn mark_health_scan_needed(db: &Database, project_id: i64) -> Result<(), String> {
    db.store_memory(
        Some(project_id),
        Some("health_scan_needed"),
        "pending",
        "system",
        Some("health"),
        1.0,
    )
    .map_err(|e| e.to_string())?;
    tracing::debug!("Marked project {} for health rescan", project_id);
    Ok(())
}

/// Scan a project for health issues
async fn scan_project_health(
    db: &Arc<Database>,
    deepseek: Option<&Arc<DeepSeekClient>>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    tracing::info!("Code health: scanning project {}", project_path);

    // Clear old health issues (run on blocking thread)
    let db_clone = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        clear_old_health_issues(&conn, project_id)
    }).await.map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    let mut total = 0;

    // Run all sync detection operations on blocking thread pool
    let db_clone = db.clone();
    let project_path_owned = project_path.to_string();
    let detection_results = tokio::task::spawn_blocking(move || -> Result<(usize, usize, usize, usize, usize, usize), String> {
        // 1. Cargo check warnings (most important)
        tracing::debug!("Code health: running cargo check for {}", project_path_owned);
        let warnings = cargo::scan_cargo_warnings(&db_clone, project_id, &project_path_owned)?;
        if warnings > 0 {
            tracing::info!("Code health: found {} cargo warnings", warnings);
        }

        // 2. TODO/FIXME comments
        let todos = detection::scan_todo_comments(&db_clone, project_id, &project_path_owned)?;
        if todos > 0 {
            tracing::info!("Code health: found {} TODOs", todos);
        }

        // 3. Unimplemented macros
        let unimpl = detection::scan_unimplemented(&db_clone, project_id, &project_path_owned)?;
        if unimpl > 0 {
            tracing::info!("Code health: found {} unimplemented! macros", unimpl);
        }

        // 4. Unused functions (from call graph)
        let unused = detection::scan_unused_functions(&db_clone, project_id)?;
        if unused > 0 {
            tracing::info!("Code health: found {} potentially unused functions", unused);
        }

        // 5. Unwrap/expect audit (panic risks)
        let unwraps = detection::scan_unwrap_usage(&db_clone, project_id, &project_path_owned)?;
        if unwraps > 0 {
            tracing::info!("Code health: found {} unwrap/expect calls in non-test code", unwraps);
        }

        // 7. Error handling quality (pattern-based)
        let error_handling = detection::scan_error_handling(&db_clone, project_id, &project_path_owned)?;
        if error_handling > 0 {
            tracing::info!("Code health: found {} error handling issues", error_handling);
        }

        Ok((warnings, todos, unimpl, unused, unwraps, error_handling))
    }).await.map_err(|e| format!("Code health detection spawn_blocking panicked: {}", e))??;

    let (warnings, todos, unimpl, unused, unwraps, error_handling) = detection_results;
    total += warnings + todos + unimpl + unused + unwraps + error_handling;

    // 6. LLM complexity analysis (for large functions) - async
    if let Some(ds) = deepseek {
        let complexity = analysis::scan_complexity(db, ds, project_id, project_path).await?;
        if complexity > 0 {
            tracing::info!("Code health: found {} complexity issues via LLM", complexity);
        }
        total += complexity;

        // 8. LLM error handling analysis (for functions with many ? operators) - async
        let error_quality = analysis::scan_error_quality(db, ds, project_id, project_path).await?;
        if error_quality > 0 {
            tracing::info!("Code health: found {} error quality issues via LLM", error_quality);
        }
        total += error_quality;
    }

    Ok(total)
}

/// Clear old health issues before refresh
fn clear_old_health_issues(conn: &rusqlite::Connection, project_id: i64) -> Result<(), String> {
    clear_old_health_issues_sync(conn, project_id).map_err(|e| e.to_string())
}
