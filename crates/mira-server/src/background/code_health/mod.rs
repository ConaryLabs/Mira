// crates/mira-server/src/background/code_health/mod.rs
// Background worker for detecting code health issues using concrete signals.
// Retained: cargo check warnings (real compiler output) and unused function detection
// (based on actual call graph data). All heuristic scoring, convention extraction,
// dependency analysis, pattern detection, and health snapshots have been removed.

mod cargo;

use crate::db::pool::DatabasePool;
use crate::db::{
    StoreObservationParams, clear_old_health_issues_sync,
    delete_observations_by_categories_sync, get_indexed_project_ids_sync,
    get_project_paths_by_ids_sync, get_unused_functions_sync, is_time_older_than_sync,
    mark_health_scanned_sync, observation_key_exists_sync, store_observation_sync,
};
use crate::utils::ResultExt;
use std::path::Path;
use std::sync::Arc;

/// Maximum unused function findings per scan
const MAX_UNUSED_FINDINGS: usize = 10;
/// Confidence for unused function findings
const CONFIDENCE_UNUSED: f64 = 0.5;

/// Find one project that needs a health scan.
/// Returns `(project_id, project_path)` if found.
async fn find_project_for_health_scan(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
) -> Result<Option<(i64, String)>, String> {
    // Get indexed project IDs from code DB
    let project_ids = code_pool.run(get_indexed_project_ids_sync).await?;

    if project_ids.is_empty() {
        return Ok(None);
    }

    // Get project paths from main DB and filter by health check needs
    let ids = project_ids;
    let result = main_pool
        .run(move |conn| {
            let all_projects = get_project_paths_by_ids_sync(conn, &ids)?;

            for (project_id, project_path) in all_projects {
                if needs_health_scan(conn, project_id) && Path::new(&project_path).exists() {
                    return Ok(Some((project_id, project_path)));
                }
            }
            Ok::<_, rusqlite::Error>(None)
        })
        .await?;

    Ok(result)
}

/// Fast scans: cargo check warnings and unused functions.
/// Clears categories: warning, unused.
/// Consumes the scan-needed flag when done.
pub async fn process_health_fast_scans(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
    tracing::debug!("Code health fast scans: checking for projects needing scan");

    let Some((project_id, project_path)) =
        find_project_for_health_scan(main_pool, code_pool).await?
    else {
        return Ok(0);
    };

    tracing::info!("Code health fast scans: scanning project {}", project_path);

    // Clear relevant categories
    main_pool
        .run(move |conn| {
            delete_observations_by_categories_sync(conn, project_id, "health", &["warning", "unused"]).map(|_| ())
        })
        .await?;

    let mut total = 0;

    // 1. Cargo check warnings.
    // Use the async tokio::process::Command variant so the Child handle is held
    // for the duration of the await. If the outer slow-lane timeout fires and
    // drops this future, tokio will kill the cargo process automatically
    // (kill_on_drop(true)), preventing orphaned cargo processes.
    let cargo_path = project_path.clone();
    tracing::debug!("Code health: running cargo check for {}", cargo_path);
    let cargo_findings = cargo::collect_cargo_warnings_async(&cargo_path)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Code health: cargo check failed: {}", e);
            Vec::new()
        });

    if !cargo_findings.is_empty() {
        tracing::info!("Code health: found {} cargo warnings", cargo_findings.len());
    }

    // Batch-write cargo findings
    let warnings_count = cargo_findings.len();
    main_pool
        .run(move |conn| {
            cargo::store_cargo_findings(conn, project_id, &cargo_findings)?;
            Ok::<_, String>(())
        })
        .await?;

    total += warnings_count;

    // 2. Unused functions
    let unused = scan_unused_functions_sharded(main_pool, code_pool, project_id).await?;
    if unused > 0 {
        tracing::info!("Code health: found {} potentially unused functions", unused);
    }
    total += unused;

    // Mark scan as complete
    main_pool
        .run(move |conn| mark_health_scanned(conn, project_id))
        .await?;

    tracing::info!(
        "Code health fast scans: found {} issues for project {}",
        total,
        project_path
    );

    Ok(total)
}

/// Run a full health scan for a specific project (cargo check + unused functions).
/// Used by the MCP `index(action="health")` tool for forced on-demand scans.
pub async fn scan_project_health_full(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    tracing::info!("Code health: full scan for project {}", project_path);

    // Clear all health issues
    main_pool
        .run(move |conn| clear_old_health_issues(conn, project_id))
        .await?;

    let mut total = 0;

    // Cargo check warnings
    let project_path_owned = project_path.to_string();
    let cargo_path = project_path_owned.clone();
    let cargo_findings =
        tokio::task::spawn_blocking(move || cargo::collect_cargo_warnings(&cargo_path))
            .await
            .map_err(|e| format!("cargo scan join error: {}", e))?
            .unwrap_or_else(|e| {
                tracing::warn!("Code health: cargo check failed: {}", e);
                Vec::new()
            });

    let warnings_count = cargo_findings.len();
    main_pool
        .run(move |conn| {
            cargo::store_cargo_findings(conn, project_id, &cargo_findings)?;
            Ok::<_, String>(())
        })
        .await?;
    total += warnings_count;

    // Unused functions
    let unused = scan_unused_functions_sharded(main_pool, code_pool, project_id).await?;
    total += unused;

    Ok(total)
}

/// Check if project needs health scanning
/// Triggers when:
/// 1. Never scanned before
/// 2. Files changed (health_scan_needed flag is set)
/// 3. Fallback: > 1 day since last scan
fn needs_health_scan(conn: &rusqlite::Connection, project_id: i64) -> bool {
    // Check if the "needs scan" flag is set (triggered by file watcher)
    if observation_key_exists_sync(conn, project_id, "health_scan_needed") {
        return true;
    }

    // Check last scan time for fallback
    let scan_info = crate::db::get_observation_info_sync(conn, project_id, "health_scan_time");

    match scan_info {
        None => true, // Never scanned
        Some((_, scan_time)) => {
            // Fallback: rescan if > 1 day old
            is_time_older_than_sync(conn, &scan_time, "-1 day")
        }
    }
}

/// Mark project as health-scanned and clear the "needs scan" flag
pub(crate) fn mark_health_scanned(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<(), String> {
    mark_health_scanned_sync(conn, project_id).str_err()
}

/// Mark project as needing a health scan (called by file watcher)
/// Sync version for pool.interact()
pub fn mark_health_scan_needed_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<(), String> {
    store_observation_sync(
        conn,
        StoreObservationParams {
            project_id: Some(project_id),
            key: Some("health_scan_needed"),
            content: "pending",
            observation_type: "system",
            category: Some("health"),
            confidence: 1.0,
            source: "code_health",
            session_id: None,
            team_id: None,
            scope: "project",
            expires_at: None,
        },
    )
    .str_err()?;
    tracing::debug!("Marked project {} for health rescan", project_id);
    Ok(())
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
        .run(move |conn| get_unused_functions_sync(conn, project_id))
        .await?;

    if unused.is_empty() {
        return Ok(0);
    }

    // Write findings to main DB
    let count = main_pool
        .run(move |conn| {
            let mut stored = 0;
            for (name, file_path, line) in &unused {
                let content = format!(
                    "[unused] Function `{}` at {}:{} appears to have no callers",
                    name, file_path, line
                );
                let key = format!("health:unused:{}:{}", file_path, name);

                store_observation_sync(
                    conn,
                    StoreObservationParams {
                        project_id: Some(project_id),
                        key: Some(&key),
                        content: &content,
                        observation_type: "health",
                        category: Some("unused"),
                        confidence: CONFIDENCE_UNUSED,
                        source: "code_health",
                        session_id: None,
                        team_id: None,
                        scope: "project",
                        expires_at: None,
                    },
                )?;

                stored += 1;
                if stored >= MAX_UNUSED_FINDINGS {
                    break;
                }
            }
            Ok::<_, rusqlite::Error>(stored)
        })
        .await?;

    Ok(count)
}

/// Clear old health issues before refresh
fn clear_old_health_issues(conn: &rusqlite::Connection, project_id: i64) -> Result<(), String> {
    clear_old_health_issues_sync(conn, project_id).str_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_main_db_with_project() -> (Connection, i64) {
        let conn = crate::db::test_support::setup_test_connection();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/health", Some("test")).unwrap();
        (conn, project_id)
    }

    #[test]
    fn test_needs_scan_never_scanned() {
        let (conn, pid) = setup_main_db_with_project();
        assert!(needs_health_scan(&conn, pid));
    }

    #[test]
    fn test_needs_scan_false_after_mark_scanned() {
        let (conn, pid) = setup_main_db_with_project();
        mark_health_scanned(&conn, pid).unwrap();
        assert!(!needs_health_scan(&conn, pid));
    }

    #[test]
    fn test_needs_scan_flag_triggers_rescan() {
        let (conn, pid) = setup_main_db_with_project();
        mark_health_scanned(&conn, pid).unwrap();
        assert!(!needs_health_scan(&conn, pid));

        mark_health_scan_needed_sync(&conn, pid).unwrap();
        assert!(needs_health_scan(&conn, pid));
    }

    #[test]
    fn test_mark_scanned_clears_needed_flag() {
        let (conn, pid) = setup_main_db_with_project();
        mark_health_scan_needed_sync(&conn, pid).unwrap();
        assert!(needs_health_scan(&conn, pid));

        mark_health_scanned(&conn, pid).unwrap();
        assert!(!needs_health_scan(&conn, pid));
    }

    #[test]
    fn test_clear_old_health_issues() {
        let (conn, pid) = setup_main_db_with_project();
        // Store some health findings in system_observations
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("health:test:issue1"),
                content: "[unused] test finding",
                observation_type: "health",
                category: Some("unused"),
                confidence: 0.5,
                source: "code_health",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM system_observations WHERE project_id = ? AND observation_type = 'health'",
                [pid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        clear_old_health_issues(&conn, pid).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM system_observations WHERE project_id = ? AND observation_type = 'health'",
                [pid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
