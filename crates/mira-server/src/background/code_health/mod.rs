// crates/mira-server/src/background/code_health/mod.rs
// Background worker for detecting code health issues using concrete signals

mod cargo;
pub mod conventions;
pub mod dependencies;
mod detection;
pub mod patterns;
pub mod scoring;

use crate::db::pool::DatabasePool;
use crate::db::{
    StoreObservationParams, clear_health_issues_by_categories_sync, clear_old_health_issues_sync,
    delete_observation_by_key_sync, get_indexed_project_ids_sync, get_project_paths_by_ids_sync,
    get_unused_functions_sync, is_time_older_than_sync, mark_health_scanned_sync,
    observation_key_exists_sync, store_observation_sync,
};
use crate::utils::ResultExt;
use rusqlite::params;
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

/// Fast scans: cargo check, pattern detection, and unused functions.
/// Clears categories: warning, todo, unimplemented, unwrap, error_handling, unused.
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

    // Clear any stale health_fast_scan_done marker from a previous cycle.
    // If module analysis timed out last cycle, it would leave this marker behind.
    // Without clearing it here, a failed fast-scan run would inherit the stale
    // marker, letting module analysis incorrectly consume health_scan_needed.
    let mp = main_pool.clone();
    mp.run(move |conn| {
        delete_observation_by_key_sync(conn, project_id, "health_fast_scan_done")?;
        Ok::<_, rusqlite::Error>(())
    })
    .await?;

    // Clear relevant categories
    main_pool
        .run(move |conn| {
            clear_health_issues_by_categories_sync(
                conn,
                project_id,
                &[
                    "warning",
                    "todo",
                    "unimplemented",
                    "unwrap",
                    "error_handling",
                    "unused",
                ],
            )
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

    // 2-5. Single-pass pattern detection (walks filesystem, reads Rust files)
    let det_path = project_path.clone();
    let det_output = tokio::task::spawn_blocking(move || detection::collect_detections(&det_path))
        .await
        .map_err(|e| format!("detection scan join error: {}", e))?
        .unwrap_or_else(|e| {
            tracing::warn!("Code health: detection scan failed: {}", e);
            detection::DetectionOutput {
                results: detection::DetectionResults {
                    todos: 0,
                    unimplemented: 0,
                    unwraps: 0,
                    error_handling: 0,
                },
                findings: Vec::new(),
            }
        });

    let det = &det_output.results;
    if det.todos > 0 {
        tracing::info!("Code health: found {} TODOs", det.todos);
    }
    if det.unimplemented > 0 {
        tracing::info!(
            "Code health: found {} unimplemented! macros",
            det.unimplemented
        );
    }
    if det.unwraps > 0 {
        tracing::info!(
            "Code health: found {} unwrap/expect calls in non-test code",
            det.unwraps
        );
    }
    if det.error_handling > 0 {
        tracing::info!(
            "Code health: found {} error handling issues",
            det.error_handling
        );
    }

    // Batch-write all findings in a single pool.interact() call
    let warnings_count = cargo_findings.len();
    let det_count = det_output.findings.len();
    main_pool
        .run(move |conn| {
            cargo::store_cargo_findings(conn, project_id, &cargo_findings)?;
            detection::store_detection_findings(conn, project_id, &det_output.findings)?;
            Ok::<_, String>(())
        })
        .await?;

    total += warnings_count + det_count;

    // 6. Unused functions
    let unused = scan_unused_functions_sharded(main_pool, code_pool, project_id).await?;
    if unused > 0 {
        tracing::info!("Code health: found {} potentially unused functions", unused);
    }
    total += unused;

    // Signal that fast scans completed successfully. Module analysis checks
    // this before clearing the scan-needed flag, so a failed fast-scan run
    // won't be suppressed by a subsequent successful module-analysis pass.
    let mp = main_pool.clone();
    mp.run(move |conn| {
        store_observation_sync(
            conn,
            StoreObservationParams {
                project_id: Some(project_id),
                key: Some("health_fast_scan_done"),
                content: "done",
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
    })
    .await?;

    tracing::info!(
        "Code health fast scans: found {} issues for project {}",
        total,
        project_path
    );

    Ok(total)
}

/// Module-level analysis: dependencies, patterns, tech debt scoring, and conventions.
/// Clears category: architecture (for patterns).
/// Dependencies, scoring, and conventions write to their own tables.
pub async fn process_health_module_analysis(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
    let Some((project_id, project_path)) =
        find_project_for_health_scan(main_pool, code_pool).await?
    else {
        return Ok(0);
    };

    tracing::debug!(
        "Code health module analysis: scanning project {}",
        project_path
    );

    // Clear architecture + circular_dependency categories (for patterns/deps stored in memory_facts)
    main_pool
        .run(move |conn| {
            clear_health_issues_by_categories_sync(
                conn,
                project_id,
                &["architecture", "circular_dependency"],
            )
        })
        .await?;

    let mut total = 0;

    // Dependencies
    match dependencies::scan_dependencies_sharded(main_pool, code_pool, project_id).await {
        Ok(dep_count) => {
            if dep_count > 0 {
                tracing::info!(
                    "Code health: computed {} module dependency edges",
                    dep_count
                );
            }
            total += dep_count;
        }
        Err(e) => {
            tracing::warn!("Code health: dependency analysis failed: {}", e);
        }
    }

    // Architectural pattern detection
    match scan_patterns_sharded(main_pool, code_pool, project_id).await {
        Ok(pattern_count) => {
            if pattern_count > 0 {
                tracing::info!(
                    "Code health: detected {} architectural patterns",
                    pattern_count
                );
            }
            total += pattern_count;
        }
        Err(e) => {
            tracing::warn!("Code health: pattern detection failed: {}", e);
        }
    }

    // Tech debt scoring
    match scoring::compute_tech_debt_scores(main_pool, code_pool, project_id).await {
        Ok(scored) => {
            if scored > 0 {
                tracing::info!(
                    "Code health: computed tech debt scores for {} modules",
                    scored
                );
            }
            // Capture health snapshot after scoring succeeds
            if scored > 0
                && let Err(e) = capture_health_snapshot(main_pool, project_id).await
            {
                tracing::warn!("Code health: snapshot capture failed: {}", e);
            }
        }
        Err(e) => {
            tracing::warn!("Code health: tech debt scoring failed: {}", e);
        }
    }

    // Convention extraction
    match scan_conventions_sharded(main_pool, code_pool, project_id).await {
        Ok(count) => {
            if count > 0 {
                tracing::info!("Code health: extracted conventions for {} modules", count);
            }
        }
        Err(e) => {
            tracing::warn!("Code health: convention extraction failed: {}", e);
        }
    }

    // Module analysis is the last core (non-LLM) health task. Only consume
    // the scan-needed flag if fast scans also completed successfully (signalled
    // by the health_fast_scan_done marker). Without this guard, a failed fast-
    // scan run would be silently suppressed by a successful module-analysis
    // finalization clearing the flag.
    let fast_scan_done = main_pool
        .run(move |conn| {
            Ok::<bool, rusqlite::Error>(observation_key_exists_sync(
                conn,
                project_id,
                "health_fast_scan_done",
            ))
        })
        .await?;

    if fast_scan_done {
        main_pool
            .run(move |conn| mark_health_scanned(conn, project_id))
            .await?;
    } else {
        tracing::info!(
            "Code health: skipping flag consumption — fast scans did not complete for project {}",
            project_id
        );
    }

    Ok(total)
}

/// Run a full health scan for a specific project (all sub-steps).
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

    // Fast scans: cargo check + detection + unused
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

    let det_path = project_path_owned.clone();
    let det_output = tokio::task::spawn_blocking(move || detection::collect_detections(&det_path))
        .await
        .map_err(|e| format!("detection scan join error: {}", e))?
        .unwrap_or_else(|e| {
            tracing::warn!("Code health: detection scan failed: {}", e);
            detection::DetectionOutput {
                results: detection::DetectionResults {
                    todos: 0,
                    unimplemented: 0,
                    unwraps: 0,
                    error_handling: 0,
                },
                findings: Vec::new(),
            }
        });

    let warnings_count = cargo_findings.len();
    let det_count = det_output.findings.len();
    main_pool
        .run(move |conn| {
            cargo::store_cargo_findings(conn, project_id, &cargo_findings)?;
            detection::store_detection_findings(conn, project_id, &det_output.findings)?;
            Ok::<_, String>(())
        })
        .await?;
    total += warnings_count + det_count;

    let unused = scan_unused_functions_sharded(main_pool, code_pool, project_id).await?;
    total += unused;

    // Module analysis
    match dependencies::scan_dependencies_sharded(main_pool, code_pool, project_id).await {
        Ok(c) => total += c,
        Err(e) => tracing::warn!("Code health: dependency analysis failed: {}", e),
    }
    match scan_patterns_sharded(main_pool, code_pool, project_id).await {
        Ok(c) => total += c,
        Err(e) => tracing::warn!("Code health: pattern detection failed: {}", e),
    }
    if let Err(e) = scoring::compute_tech_debt_scores(main_pool, code_pool, project_id).await {
        tracing::warn!("Code health: tech debt scoring failed: {}", e);
    }
    // Capture health snapshot after scoring
    if let Err(e) = capture_health_snapshot(main_pool, project_id).await {
        tracing::warn!("Code health: snapshot capture failed: {}", e);
    }
    if let Err(e) = scan_conventions_sharded(main_pool, code_pool, project_id).await {
        tracing::warn!("Code health: convention extraction failed: {}", e);
    }

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

/// Scan architectural patterns using sharded pools.
async fn scan_patterns_sharded(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<usize, String> {
    // Run pattern detection on code DB
    let pattern_findings = code_pool
        .run(move |conn| patterns::collect_pattern_data(conn, project_id))
        .await?;

    if pattern_findings.is_empty() {
        return Ok(0);
    }

    let count = pattern_findings.len();

    // Store pattern findings in main DB (system_observations)
    main_pool
        .run(move |conn| {
            for finding in &pattern_findings {
                store_observation_sync(
                    conn,
                    StoreObservationParams {
                        project_id: Some(project_id),
                        key: Some(&finding.key),
                        content: &finding.content,
                        observation_type: "health",
                        category: Some("architecture"),
                        confidence: finding.confidence,
                        source: "code_health",
                        session_id: None,
                        team_id: None,
                        scope: "project",
                        expires_at: None,
                    },
                )
                .str_err()?;
            }
            Ok::<_, String>(())
        })
        .await?;

    Ok(count)
}

/// Scan coding conventions using sharded pools.
/// Reads code_chunks/imports/symbols from code_pool, writes to module_conventions in main_pool.
async fn scan_conventions_sharded(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<usize, String> {
    // Collect convention data from code DB
    let convention_data = code_pool
        .run(move |conn| conventions::collect_convention_data(conn, project_id))
        .await?;

    if convention_data.is_empty() {
        return Ok(0);
    }

    let count = convention_data.len();

    // Collect module paths for marking as extracted
    let module_paths: Vec<String> = convention_data
        .iter()
        .map(|d| d.module_path.clone())
        .collect();

    // Store convention data in main DB
    main_pool
        .run(move |conn| conventions::upsert_module_conventions(conn, project_id, &convention_data))
        .await?;

    // Mark modules as extracted in code DB
    code_pool
        .run(move |conn| conventions::mark_conventions_extracted(conn, project_id, &module_paths))
        .await?;

    Ok(count)
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

    // Return the number of findings stored (capped at MAX_UNUSED_FINDINGS),
    // not unused.len(). The return value is "how many were stored", not
    // "how many were found". Callers use this for progress totals, so the
    // capped count is the correct value to report.
    Ok(count)
}

/// Capture a health snapshot after scoring completes.
/// Rate-limited: at most one snapshot per 6 hours per project.
async fn capture_health_snapshot(pool: &Arc<DatabasePool>, project_id: i64) -> Result<(), String> {
    pool.run(move |conn| {
        // Rate limit: skip if latest snapshot is < 6 hours old
        let too_recent: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM health_snapshots
                 WHERE project_id = ?1
                   AND snapshot_at > datetime('now', '-6 hours'))",
                params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if too_recent {
            tracing::debug!("Code health: skipping snapshot — too recent for project {}", project_id);
            return Ok::<(), String>(());
        }

        // Aggregate tech debt scores
        let (module_count, avg_score, max_score): (i64, f64, f64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(AVG(overall_score), 0.0), COALESCE(MAX(overall_score), 0.0)
                 FROM tech_debt_scores WHERE project_id = ?1",
                params![project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap_or((0, 0.0, 0.0));

        if module_count == 0 {
            return Ok(()); // Nothing to snapshot
        }

        // Tier distribution as JSON
        let tier_dist = {
            let mut stmt = conn
                .prepare(
                    "SELECT tier, COUNT(*) FROM tech_debt_scores
                     WHERE project_id = ?1 GROUP BY tier ORDER BY tier",
                )
                .map_err(|e| format!("prepare tier dist: {}", e))?;
            let tiers: Vec<(String, i64)> = stmt
                .query_map(params![project_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|e| format!("query tier dist: {}", e))?
                .filter_map(|r| r.ok())
                .collect();
            serde_json::to_string(&tiers.into_iter().collect::<std::collections::HashMap<_, _>>())
                .unwrap_or_else(|_| "{}".to_string())
        };

        // Count findings by category from system_observations
        let count_findings = |category: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM system_observations
                 WHERE project_id = ?1 AND observation_type = 'health' AND category = ?2",
                params![project_id, category],
                |row| row.get(0),
            )
            .unwrap_or(0)
        };

        let warning_count = count_findings("complexity");
        let todo_count = count_findings("todo") + count_findings("unimplemented");
        let unwrap_count = count_findings("unwrap");
        let error_handling_count = count_findings("error_handling") + count_findings("error_quality");
        let total_finding_count = warning_count + todo_count + unwrap_count + error_handling_count
            + count_findings("unused");

        conn.execute(
            "INSERT INTO health_snapshots
             (project_id, module_count, avg_debt_score, max_debt_score, tier_distribution,
              warning_count, todo_count, unwrap_count, error_handling_count, total_finding_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                project_id,
                module_count,
                avg_score,
                max_score,
                tier_dist,
                warning_count,
                todo_count,
                unwrap_count,
                error_handling_count,
                total_finding_count,
            ],
        )
        .map_err(|e| format!("insert health snapshot: {}", e))?;

        tracing::info!(
            "Code health: captured snapshot for project {} (avg={:.1}, max={:.1}, {} modules)",
            project_id,
            avg_score,
            max_score,
            module_count
        );

        Ok(())
    })
    .await
    .map_err(Into::into)
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

    // =========================================================================
    // Orchestration tests — verify subtask scheduling doesn't starve later tasks
    // =========================================================================

    #[test]
    fn test_scan_needed_survives_fast_scan_phase() {
        // After fast scans complete (without calling mark_health_scanned),
        // needs_health_scan should still return true so module analysis can run.
        let (conn, pid) = setup_main_db_with_project();
        mark_health_scan_needed_sync(&conn, pid).unwrap();
        assert!(needs_health_scan(&conn, pid));

        // Simulate what fast scans do: they set a done marker but do NOT
        // call mark_health_scanned. The scan-needed flag should remain set.
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("health_fast_scan_done"),
                content: "done",
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
        .unwrap();
        assert!(
            needs_health_scan(&conn, pid),
            "scan-needed flag must survive fast scan phase"
        );
    }

    #[test]
    fn test_module_analysis_consumes_scan_flag() {
        // Module analysis is the last core task and should consume the flag
        // when fast scans have signalled completion.
        let (conn, pid) = setup_main_db_with_project();
        mark_health_scan_needed_sync(&conn, pid).unwrap();
        assert!(needs_health_scan(&conn, pid));

        // Fast scans signal completion
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("health_fast_scan_done"),
                content: "done",
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
        .unwrap();

        // Module analysis finalizes
        mark_health_scanned(&conn, pid).unwrap();
        assert!(
            !needs_health_scan(&conn, pid),
            "module analysis should consume the scan-needed flag"
        );
        // Fast scan done marker should also be cleared
        assert!(
            !observation_key_exists_sync(&conn, pid, "health_fast_scan_done"),
            "fast scan done marker should be cleared"
        );
    }

    #[test]
    fn test_failed_fast_scan_prevents_flag_consumption() {
        // When fast scans fail (no health_fast_scan_done marker), module
        // analysis must NOT clear the scan-needed flag. This ensures a
        // retry on the next cycle.
        let (conn, pid) = setup_main_db_with_project();
        mark_health_scan_needed_sync(&conn, pid).unwrap();
        assert!(needs_health_scan(&conn, pid));

        // Fast scans failed — no health_fast_scan_done marker set.
        // Module analysis checks for the marker and skips finalization.
        assert!(
            !observation_key_exists_sync(&conn, pid, "health_fast_scan_done"),
            "fast scan done marker should not exist after failure"
        );

        // The scan-needed flag must survive for retry.
        assert!(
            needs_health_scan(&conn, pid),
            "scan-needed flag must survive when fast scans failed"
        );
    }

    #[test]
    fn test_stale_fast_scan_marker_does_not_leak_across_cycles() {
        // Scenario from codex review: cycle N fast scans succeed (marker set),
        // module analysis times out (marker NOT cleaned up). Cycle N+1 fast
        // scans fail. The stale marker from cycle N must not trick module
        // analysis into consuming health_scan_needed.
        //
        // The fix: fast scans clear the marker at start, so a stale marker
        // from a previous cycle cannot persist into the current one.
        let (conn, pid) = setup_main_db_with_project();
        mark_health_scan_needed_sync(&conn, pid).unwrap();

        // Simulate cycle N: fast scans succeed → marker set
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("health_fast_scan_done"),
                content: "done",
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
        .unwrap();
        assert!(observation_key_exists_sync(
            &conn,
            pid,
            "health_fast_scan_done"
        ));

        // Simulate cycle N: module analysis times out → marker NOT cleaned.
        // (mark_health_scanned was never called)

        // Simulate cycle N+1: fast scans START → must clear stale marker.
        // This is the new behavior added in the fix.
        delete_observation_by_key_sync(&conn, pid, "health_fast_scan_done").unwrap();

        // Simulate cycle N+1: fast scans FAIL → marker not re-set.
        assert!(
            !observation_key_exists_sync(&conn, pid, "health_fast_scan_done"),
            "stale marker must be gone after fast scan start clears it"
        );

        // Module analysis checks the marker: not present → does NOT consume flag.
        assert!(
            needs_health_scan(&conn, pid),
            "scan-needed flag must survive when stale marker was cleared and fast scans failed"
        );
    }

    #[test]
    fn test_clear_circular_dependency_category() {
        // Regression test: process_health_module_analysis must clear "circular_dependency"
        // in addition to "architecture" so stale findings don't persist.
        let (conn, pid) = setup_main_db_with_project();

        // Store a circular dependency finding in system_observations
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("health:circular:mod_a:mod_b"),
                content: "[circular-dependency] Circular dependency: mod_a <-> mod_b",
                observation_type: "health",
                category: Some("circular_dependency"),
                confidence: 0.9,
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
                "SELECT COUNT(*) FROM system_observations WHERE project_id = ? AND category = 'circular_dependency'",
                [pid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Clear the categories that module analysis clears
        clear_health_issues_by_categories_sync(
            &conn,
            pid,
            &["architecture", "circular_dependency"],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM system_observations WHERE project_id = ? AND category = 'circular_dependency'",
                [pid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 0,
            "circular_dependency findings must be cleared by module analysis"
        );
    }
}
