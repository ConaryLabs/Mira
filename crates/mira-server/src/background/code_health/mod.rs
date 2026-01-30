// crates/mira-server/src/background/code_health/mod.rs
// Background worker for detecting code health issues using concrete signals

mod analysis;
mod cargo;
mod detection;

use crate::db::pool::DatabasePool;
use crate::db::{
    StoreMemoryParams, clear_old_health_issues_sync, get_indexed_project_ids_sync,
    get_project_paths_by_ids_sync, get_scan_info_sync, get_unused_functions_sync,
    is_time_older_than_sync, mark_health_scanned_sync, memory_key_exists_sync, store_memory_sync,
};
use crate::llm::LlmClient;
use crate::utils::ResultExt;
use std::path::Path;
use std::sync::Arc;

/// Maximum unused function findings per scan
const MAX_UNUSED_FINDINGS: usize = 10;
/// Confidence for unused function findings
const CONFIDENCE_UNUSED: f64 = 0.5;

/// Check code health for all indexed projects.
///
/// - `main_pool`: for reading/writing memory_facts, health markers
/// - `code_pool`: for reading code_symbols, call_graph
pub async fn process_code_health(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
) -> Result<usize, String> {
    tracing::debug!("Code health: checking for projects needing scan");

    // Step 1: Get indexed project IDs from code DB
    let project_ids = code_pool
        .interact(move |conn| {
            get_indexed_project_ids_sync(conn).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    if project_ids.is_empty() {
        return Ok(0);
    }

    // Step 2: Get project paths from main DB and filter by health check needs
    let ids = project_ids.clone();
    let projects = main_pool
        .interact(move |conn| {
            let all_projects =
                get_project_paths_by_ids_sync(conn, &ids).map_err(|e| anyhow::anyhow!("{}", e))?;

            let mut needing_scan = Vec::new();
            for (project_id, project_path) in all_projects {
                if needs_health_scan(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))? {
                    needing_scan.push((project_id, project_path));
                    break; // One project per cycle
                }
            }
            Ok::<_, anyhow::Error>(needing_scan)
        })
        .await
        .str_err()?;

    if !projects.is_empty() {
        tracing::info!(
            "Code health: found {} projects needing scan",
            projects.len()
        );
    }

    let mut processed = 0;

    for (project_id, project_path) in projects {
        if !Path::new(&project_path).exists() {
            continue;
        }

        match scan_project_health(main_pool, code_pool, client, project_id, &project_path).await {
            Ok(count) => {
                tracing::info!(
                    "Found {} health issues for project {} ({})",
                    count,
                    project_id,
                    project_path
                );
                processed += count;
                main_pool
                    .interact(move |conn| {
                        mark_health_scanned(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
                    })
                    .await
                    .str_err()?;
            }
            Err(e) => {
                tracing::warn!("Failed to scan health for {}: {}", project_path, e);
            }
        }
    }

    Ok(processed)
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
    mark_health_scanned_sync(conn, project_id).str_err()
}

/// Mark project as needing a health scan (called by file watcher)
/// Sync version for pool.interact()
pub fn mark_health_scan_needed_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<(), String> {
    store_memory_sync(
        conn,
        StoreMemoryParams {
            project_id: Some(project_id),
            key: Some("health_scan_needed"),
            content: "pending",
            fact_type: "system",
            category: Some("health"),
            confidence: 1.0,
            session_id: None,
            user_id: None,
            scope: "project",
            branch: None,
        },
    )
    .str_err()?;
    tracing::debug!("Marked project {} for health rescan", project_id);
    Ok(())
}

/// Scan a project for health issues.
///
/// - `main_pool`: for writing findings to memory_facts
/// - `code_pool`: for reading code_symbols/call_graph
async fn scan_project_health(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    tracing::info!("Code health: scanning project {}", project_path);

    // Clear old health issues (main DB)
    main_pool
        .interact(move |conn| {
            clear_old_health_issues(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    let mut total = 0;

    // Run file-based detection operations using main_pool (they only write to memory_facts)
    let project_path_owned = project_path.to_string();
    let detection_results = main_pool
        .interact(
            move |conn| -> Result<(usize, usize, usize, usize, usize), anyhow::Error> {
                // 1. Cargo check warnings (most important)
                tracing::debug!(
                    "Code health: running cargo check for {}",
                    project_path_owned
                );
                let warnings = cargo::scan_cargo_warnings(conn, project_id, &project_path_owned)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if warnings > 0 {
                    tracing::info!("Code health: found {} cargo warnings", warnings);
                }

                // 2. TODO/FIXME comments
                let todos = detection::scan_todo_comments(conn, project_id, &project_path_owned)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if todos > 0 {
                    tracing::info!("Code health: found {} TODOs", todos);
                }

                // 3. Unimplemented macros
                let unimpl = detection::scan_unimplemented(conn, project_id, &project_path_owned)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if unimpl > 0 {
                    tracing::info!("Code health: found {} unimplemented! macros", unimpl);
                }

                // 4. Unwrap/expect audit (panic risks)
                let unwraps = detection::scan_unwrap_usage(conn, project_id, &project_path_owned)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if unwraps > 0 {
                    tracing::info!(
                        "Code health: found {} unwrap/expect calls in non-test code",
                        unwraps
                    );
                }

                // 5. Error handling quality (pattern-based)
                let error_handling =
                    detection::scan_error_handling(conn, project_id, &project_path_owned)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                if error_handling > 0 {
                    tracing::info!(
                        "Code health: found {} error handling issues",
                        error_handling
                    );
                }

                Ok((warnings, todos, unimpl, unwraps, error_handling))
            },
        )
        .await
        .str_err()?;

    let (warnings, todos, unimpl, unwraps, error_handling) = detection_results;
    total += warnings + todos + unimpl + unwraps + error_handling;

    // 6. Unused functions - reads from code DB (code_symbols/call_graph),
    //    writes findings to main DB (memory_facts)
    let unused = scan_unused_functions_sharded(main_pool, code_pool, project_id).await?;
    if unused > 0 {
        tracing::info!("Code health: found {} potentially unused functions", unused);
    }
    total += unused;

    // 7. LLM complexity analysis (for large functions) - async
    if let Some(llm) = client {
        let complexity =
            analysis::scan_complexity(code_pool, main_pool, llm, project_id, project_path).await?;
        if complexity > 0 {
            tracing::info!(
                "Code health: found {} complexity issues via LLM",
                complexity
            );
        }
        total += complexity;

        // 8. LLM error handling analysis (for functions with many ? operators) - async
        let error_quality =
            analysis::scan_error_quality(code_pool, main_pool, llm, project_id, project_path)
                .await?;
        if error_quality > 0 {
            tracing::info!(
                "Code health: found {} error quality issues via LLM",
                error_quality
            );
        }
        total += error_quality;
    }

    Ok(total)
}

/// Scan for unused functions using sharded pools.
/// Reads code_symbols/call_graph from code_pool, writes findings to main_pool.
async fn scan_unused_functions_sharded(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<usize, String> {
    // Read unused functions from code DB
    let unused = code_pool
        .interact(move |conn| {
            get_unused_functions_sync(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    if unused.is_empty() {
        return Ok(0);
    }

    // Write findings to main DB
    let count = main_pool
        .interact(move |conn| -> Result<usize, anyhow::Error> {
            let mut stored = 0;
            for (name, file_path, line) in &unused {
                let content = format!(
                    "[unused] Function `{}` at {}:{} appears to have no callers",
                    name, file_path, line
                );
                let key = format!("health:unused:{}:{}", file_path, name);

                store_memory_sync(
                    conn,
                    StoreMemoryParams {
                        project_id: Some(project_id),
                        key: Some(&key),
                        content: &content,
                        fact_type: "health",
                        category: Some("unused"),
                        confidence: CONFIDENCE_UNUSED,
                        session_id: None,
                        user_id: None,
                        scope: "project",
                        branch: None,
                    },
                )
                .map_err(|e| anyhow::anyhow!("{}", e))?;

                stored += 1;
                if stored >= MAX_UNUSED_FINDINGS {
                    break;
                }
            }
            Ok(stored)
        })
        .await
        .str_err()?;

    Ok(count)
}

/// Clear old health issues before refresh
fn clear_old_health_issues(conn: &rusqlite::Connection, project_id: i64) -> Result<(), String> {
    clear_old_health_issues_sync(conn, project_id).str_err()
}
