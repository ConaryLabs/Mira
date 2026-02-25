// background/pondering/queries.rs
// Database queries for pondering data gathering

use super::types::{
    FragileModule, HealthTrend, ProjectInsightData, RecurringError, RevertCluster, StaleGoal,
    ToolUsageEntry, TrendDirection,
};
use crate::db::pool::DatabasePool;
use crate::utils::truncate;
use rusqlite::params;
use std::collections::HashMap;
use std::sync::Arc;

/// Maximum tool history entries to analyze per batch
const MAX_HISTORY_ENTRIES: usize = 100;

/// Hours to look back for recent activity
const LOOKBACK_HOURS: i64 = 24;

/// Get recent tool usage history for a project
pub(super) async fn get_recent_tool_history(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<ToolUsageEntry>, String> {
    pool.run(move |conn| {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT th.tool_name, th.arguments, th.success, th.created_at
                FROM tool_history th
                JOIN sessions s ON s.id = th.session_id
                WHERE s.project_id = ?
                  AND th.created_at > datetime('now', '-' || ? || ' hours')
                ORDER BY th.created_at DESC
                LIMIT ?
            "#,
            )
            .map_err(|e| anyhow::anyhow!("Failed to prepare: {}", e))?;

        let rows = stmt
            .query_map(
                params![project_id, LOOKBACK_HOURS, MAX_HISTORY_ENTRIES],
                |row| {
                    let args: Option<String> = row.get(1)?;
                    Ok(ToolUsageEntry {
                        tool_name: row.get(0)?,
                        arguments_summary: summarize_arguments(&args.unwrap_or_default()),
                        success: row.get::<_, i32>(2)? == 1,
                        timestamp: row.get(3)?,
                    })
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to query: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Failed to collect: {}", e))
    })
    .await
    .map_err(Into::into)
}

/// Summarize tool arguments to avoid leaking sensitive data
pub(super) fn summarize_arguments(args: &str) -> String {
    // Parse JSON and extract just the keys/structure
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(args)
        && let Some(obj) = value.as_object()
    {
        let keys: Vec<&str> = obj.keys().map(|s| s.as_str()).collect();
        return format!("keys: {}", keys.join(", "));
    }
    // Fallback: truncate
    truncate(args, 50)
}

// ── Project-aware queries ──────────────────────────────────────────────

/// Goals with `status = 'in_progress'` that haven't been updated recently.
pub(super) async fn get_stale_goals(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<StaleGoal>, String> {
    pool.run(move |conn| {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT
                    g.id,
                    g.title,
                    g.status,
                    g.progress_percent,
                    CAST(julianday('now') - julianday(COALESCE(g.updated_at, g.created_at)) AS INTEGER) AS days_since_update,
                    COALESCE(m_total.cnt, 0) AS milestones_total,
                    COALESCE(m_done.cnt, 0) AS milestones_completed
                FROM goals g
                LEFT JOIN (
                    SELECT goal_id, COUNT(*) AS cnt
                    FROM milestones
                    GROUP BY goal_id
                ) m_total ON m_total.goal_id = g.id
                LEFT JOIN (
                    SELECT goal_id, COUNT(*) AS cnt
                    FROM milestones
                    WHERE completed = 1
                    GROUP BY goal_id
                ) m_done ON m_done.goal_id = g.id
                WHERE g.project_id = ?
                  AND g.status = 'in_progress'
                  AND COALESCE(g.updated_at, g.created_at) < datetime('now', '-14 days')
                ORDER BY days_since_update DESC
            "#,
            )
            .map_err(|e| anyhow::anyhow!("Failed to prepare stale goals: {}", e))?;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(StaleGoal {
                    goal_id: row.get(0)?,
                    title: row.get(1)?,
                    status: row.get(2)?,
                    progress_percent: row.get(3)?,
                    days_since_update: row.get(4)?,
                    milestones_total: row.get(5)?,
                    milestones_completed: row.get(6)?,
                })
            })
            .map_err(|e| anyhow::anyhow!("Failed to query stale goals: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Failed to collect stale goals: {}", e))
    })
    .await
    .map_err(Into::into)
}

/// Modules where a significant portion of diffs resulted in reverts or follow-up fixes.
/// Groups by top-level directory extracted from `files_json`.
pub(super) async fn get_fragile_modules(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<FragileModule>, String> {
    pool.run(move |conn| {
        // Fetch all diff analyses with their files and any outcomes
        let mut stmt = conn
            .prepare(
                r#"
                SELECT
                    da.id,
                    da.files_json,
                    do_out.outcome_type
                FROM diff_analyses da
                LEFT JOIN diff_outcomes do_out ON do_out.diff_analysis_id = da.id
                WHERE da.project_id = ?
                  AND da.created_at > datetime('now', '-30 days')
                  AND da.files_json IS NOT NULL
                ORDER BY da.id
            "#,
            )
            .map_err(|e| anyhow::anyhow!("Failed to prepare fragile modules: {}", e))?;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| anyhow::anyhow!("Failed to query fragile modules: {}", e))?;

        // Track per-module stats: (total_diffs, reverted, follow_up_fixes)
        let mut module_stats: HashMap<String, (i64, i64, i64)> = HashMap::new();
        // Track which diff IDs we've already counted per module for totals
        let mut seen_diffs: HashMap<String, std::collections::HashSet<i64>> = HashMap::new();

        for row in rows {
            let (diff_id, files_json, outcome_type) =
                row.map_err(|e| anyhow::anyhow!("Row error: {}", e))?;

            let modules = extract_modules_from_files_json(&files_json.unwrap_or_default());

            for module in modules {
                let entry = module_stats.entry(module.clone()).or_insert((0, 0, 0));
                let seen = seen_diffs.entry(module).or_default();
                if seen.insert(diff_id) {
                    entry.0 += 1; // total_changes
                }

                if let Some(ref ot) = outcome_type {
                    match ot.as_str() {
                        "revert" => entry.1 += 1,
                        "follow_up_fix" | "fix" => entry.2 += 1,
                        _ => {}
                    }
                }
            }
        }

        let mut results: Vec<FragileModule> = module_stats
            .into_iter()
            .filter_map(|(module, (total, reverted, fixes))| {
                if total < 3 {
                    return None; // Need minimum sample size
                }
                let bad = reverted + fixes;
                let bad_rate = bad as f64 / total as f64;
                if bad_rate > 0.3 {
                    Some(FragileModule {
                        module,
                        total_changes: total,
                        reverted,
                        follow_up_fixes: fixes,
                        bad_rate,
                    })
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| {
            b.bad_rate
                .partial_cmp(&a.bad_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok::<_, anyhow::Error>(results)
    })
    .await
    .map_err(Into::into)
}

/// 2+ reverts in the same module within 48h, looking back 7 days.
pub(super) async fn get_recent_revert_clusters(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<RevertCluster>, String> {
    pool.run(move |conn| {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT
                    da.files_json,
                    do_out.evidence_commit,
                    do_out.created_at,
                    CAST(strftime('%s', do_out.created_at) AS INTEGER) AS epoch_secs
                FROM diff_outcomes do_out
                JOIN diff_analyses da ON da.id = do_out.diff_analysis_id
                WHERE do_out.project_id = ?
                  AND do_out.outcome_type = 'revert'
                  AND do_out.created_at > datetime('now', '-7 days')
                ORDER BY do_out.created_at ASC
            "#,
            )
            .map_err(|e| anyhow::anyhow!("Failed to prepare revert clusters: {}", e))?;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| anyhow::anyhow!("Failed to query revert clusters: {}", e))?;

        // Group reverts by module with epoch seconds for accurate timespan
        let mut module_reverts: HashMap<String, Vec<(String, i64)>> = HashMap::new();

        for row in rows {
            let (files_json, commit, _timestamp, epoch_secs) =
                row.map_err(|e| anyhow::anyhow!("Row error: {}", e))?;

            let modules = extract_modules_from_files_json(&files_json.unwrap_or_default());
            let commit = commit.unwrap_or_default();

            for module in modules {
                module_reverts
                    .entry(module)
                    .or_default()
                    .push((commit.clone(), epoch_secs));
            }
        }

        // Find clusters: 2+ reverts within 48h windows
        let mut clusters = Vec::new();

        for (module, mut reverts) in module_reverts {
            if reverts.len() < 2 {
                continue;
            }

            reverts.sort_by_key(|(_, secs)| *secs);

            let commits: Vec<String> = reverts.iter().map(|(c, _)| c.clone()).collect();
            let count = reverts.len() as i64;

            // Compute actual timespan using SQLite-provided epoch seconds
            let first_secs = reverts[0].1;
            let last_secs = reverts[reverts.len() - 1].1;
            let timespan_hours = (last_secs - first_secs) / 3600;

            // Only include as a cluster if reverts are actually clustered (within 48h)
            if timespan_hours <= 48 {
                clusters.push(RevertCluster {
                    module,
                    revert_count: count,
                    timespan_hours,
                    commits,
                });
            }
        }

        clusters.sort_by(|a, b| b.revert_count.cmp(&a.revert_count));
        Ok::<_, anyhow::Error>(clusters)
    })
    .await
    .map_err(Into::into)
}

/// Errors that recur across multiple sessions without resolution (3+ occurrences).
pub(super) async fn get_recurring_errors(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<RecurringError>, String> {
    pool.run(move |conn| {
        let mut stmt = conn
            .prepare(
                r#"SELECT tool_name, error_template, occurrence_count,
                          first_seen_session_id, last_seen_session_id
                   FROM error_patterns
                   WHERE project_id = ?
                     AND resolved_at IS NULL
                     AND occurrence_count >= 3
                   ORDER BY occurrence_count DESC
                   LIMIT 10"#,
            )
            .map_err(|e| anyhow::anyhow!("Failed to prepare recurring errors: {}", e))?;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(RecurringError {
                    tool_name: row.get(0)?,
                    error_template: row.get(1)?,
                    occurrence_count: row.get(2)?,
                    first_seen_session_id: row.get(3)?,
                    last_seen_session_id: row.get(4)?,
                })
            })
            .map_err(|e| anyhow::anyhow!("Failed to query recurring errors: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Failed to collect recurring errors: {}", e))
    })
    .await
    .map_err(Into::into)
}

/// Get health trend from recent snapshots for a project.
pub(super) async fn get_health_trend(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Option<HealthTrend>, String> {
    pool.run(move |conn| {
        // Get 2 most recent snapshots
        let mut stmt = conn
            .prepare(
                "SELECT avg_debt_score, tier_distribution
                 FROM health_snapshots
                 WHERE project_id = ?1
                 ORDER BY snapshot_at DESC
                 LIMIT 2",
            )
            .map_err(|e| anyhow::anyhow!("prepare health trend: {}", e))?;

        let snapshots: Vec<(f64, String)> = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, f64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| anyhow::anyhow!("query health trend: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        if snapshots.is_empty() {
            return Ok::<_, anyhow::Error>(None);
        }

        let current_score = snapshots[0].0;
        let current_tier_dist = snapshots[0].1.clone();
        let previous_score = snapshots.get(1).map(|s| s.0);

        // 7-day average
        let week_avg: Option<f64> = conn
            .query_row(
                "SELECT AVG(avg_debt_score) FROM health_snapshots
                 WHERE project_id = ?1
                   AND snapshot_at > datetime('now', '-7 days')",
                params![project_id],
                |row| row.get(0),
            )
            .ok();

        // Snapshot count
        let snapshot_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM health_snapshots WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Determine direction: >10% change threshold
        let direction = match previous_score {
            Some(prev) if prev > 0.0 => {
                let delta_pct = ((current_score - prev) / prev) * 100.0;
                if delta_pct > 10.0 {
                    TrendDirection::Degrading // higher score = worse
                } else if delta_pct < -10.0 {
                    TrendDirection::Improving // lower score = better
                } else {
                    TrendDirection::Stable
                }
            }
            _ => TrendDirection::Stable,
        };

        Ok(Some(HealthTrend {
            current_score,
            previous_score,
            week_avg_score: week_avg,
            current_tier_dist,
            snapshot_count,
            direction,
        }))
    })
    .await
    .map_err(Into::into)
}

/// Gather all project insight data in one call.
pub(super) async fn get_project_insight_data(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<ProjectInsightData, String> {
    // Run all queries concurrently
    let (stale_goals, fragile_modules, revert_clusters, recurring_errors, health_trend) =
        tokio::join!(
            get_stale_goals(pool, project_id),
            get_fragile_modules(pool, project_id),
            get_recent_revert_clusters(pool, project_id),
            get_recurring_errors(pool, project_id),
            get_health_trend(pool, project_id),
        );

    Ok(ProjectInsightData {
        stale_goals: stale_goals.unwrap_or_default(),
        fragile_modules: fragile_modules.unwrap_or_default(),
        revert_clusters: revert_clusters.unwrap_or_default(),
        recurring_errors: recurring_errors.unwrap_or_default(),
        health_trend: health_trend.unwrap_or(None),
    })
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Extract top-level module names from a files_json JSON array.
/// e.g. `["src/db/pool.rs", "src/db/tasks.rs"]` -> `{"src/db"}`.
fn extract_modules_from_files_json(files_json: &str) -> Vec<String> {
    let paths: Vec<String> = serde_json::from_str(files_json).unwrap_or_default();
    let mut modules: std::collections::HashSet<String> = std::collections::HashSet::new();

    for path in &paths {
        // Use the first two path components as the module identifier
        let parts: Vec<&str> = path.split('/').collect();
        let module = if parts.len() >= 2 {
            format!("{}/{}", parts[0], parts[1])
        } else {
            parts[0].to_string()
        };
        modules.insert(module);
    }

    modules.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_arguments() {
        let args = r#"{"file_path": "/secret/path", "query": "password"}"#;
        let summary = summarize_arguments(args);
        assert!(summary.contains("file_path"));
        assert!(!summary.contains("/secret/path"));
    }

    #[test]
    fn test_extract_modules_from_files_json() {
        let json = r#"["src/db/pool.rs", "src/db/tasks.rs", "src/background/pondering.rs"]"#;
        let mut modules = extract_modules_from_files_json(json);
        modules.sort();
        assert_eq!(modules, vec!["src/background", "src/db"]);
    }

    #[test]
    fn test_extract_modules_empty() {
        assert!(extract_modules_from_files_json("").is_empty());
        assert!(extract_modules_from_files_json("[]").is_empty());
    }

    #[test]
    fn test_extract_modules_single_component() {
        let json = r#"["Cargo.toml"]"#;
        let modules = extract_modules_from_files_json(json);
        assert_eq!(modules, vec!["Cargo.toml"]);
    }

    // ── Edge-case tests for helper functions ─────────────────────────────

    #[test]
    fn test_summarize_arguments_empty_string() {
        let summary = summarize_arguments("");
        // Empty string is not valid JSON, should truncate (returning empty)
        assert!(summary.is_empty() || summary.len() <= 50);
    }

    #[test]
    fn test_summarize_arguments_non_json() {
        let summary = summarize_arguments("this is not json at all");
        // Should fall back to truncation
        assert_eq!(summary, "this is not json at all");
    }

    #[test]
    fn test_summarize_arguments_json_array_not_object() {
        // JSON array, not an object -- should fall back to truncation
        let summary = summarize_arguments(r#"[1, 2, 3]"#);
        assert_eq!(summary, "[1, 2, 3]");
    }

    #[test]
    fn test_summarize_arguments_long_string_truncated() {
        let long = "x".repeat(200);
        let summary = summarize_arguments(&long);
        assert!(summary.len() <= 53); // truncate(s, 50) + "..."
    }

    #[test]
    fn test_summarize_arguments_empty_json_object() {
        let summary = summarize_arguments("{}");
        assert_eq!(summary, "keys: ");
    }

    #[test]
    fn test_extract_modules_malformed_json() {
        // Malformed JSON should return empty, not panic
        let modules = extract_modules_from_files_json("{not valid json}");
        assert!(modules.is_empty());
    }

    #[test]
    fn test_extract_modules_json_with_non_string_elements() {
        // JSON array with non-string elements
        let modules = extract_modules_from_files_json("[1, 2, null]");
        assert!(modules.is_empty());
    }

    #[test]
    fn test_extract_modules_deeply_nested_paths() {
        let json = r#"["a/b/c/d/e.rs"]"#;
        let modules = extract_modules_from_files_json(json);
        // Should take first two components: "a/b"
        assert_eq!(modules, vec!["a/b"]);
    }

    // ── Async DB tests for query functions on empty tables ───────────────

    #[tokio::test]
    async fn test_get_recent_tool_history_empty_tables() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        let result = get_recent_tool_history(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_stale_goals_empty_tables() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        let result = get_stale_goals(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_fragile_modules_empty_tables() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        let result = get_fragile_modules(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_recent_revert_clusters_empty_tables() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        let result = get_recent_revert_clusters(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_recurring_errors_empty_tables() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        let result = get_recurring_errors(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_health_trend_empty_tables() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        let result = get_health_trend(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_get_project_insight_data_empty_tables() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        let result = get_project_insight_data(&pool, project_id).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert!(!data.has_data());
    }

    #[tokio::test]
    async fn test_get_recent_tool_history_wrong_project() {
        use crate::db::test_support::{
            seed_session, seed_tool_history, setup_test_pool_with_project,
        };
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Seed data for the real project
        pool.run(move |conn| {
            seed_session(conn, "sess-1", project_id, "active");
            seed_tool_history(conn, "sess-1", "Read", r#"{"file_path": "test.rs"}"#, "ok");
            Ok::<_, anyhow::Error>(())
        })
        .await
        .unwrap();

        // Query with a non-existent project ID -- should return empty
        let result = get_recent_tool_history(&pool, 99999).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_fragile_modules_malformed_files_json() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Insert a diff_analysis row with malformed files_json
        pool.run(move |conn| {
            conn.execute(
                "INSERT INTO diff_analyses (project_id, from_commit, to_commit, analysis_type, files_json, created_at)
                 VALUES (?, 'abc', 'def', 'commit', '{not valid json}', datetime('now'))",
                rusqlite::params![project_id],
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .unwrap();

        // Should not panic, malformed JSON is handled gracefully
        let result = get_fragile_modules(&pool, project_id).await;
        assert!(result.is_ok());
        // Malformed JSON -> extract_modules returns empty -> no modules tracked
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_fragile_modules_null_files_json() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Insert a diff_analysis row with NULL files_json (filtered by query)
        pool.run(move |conn| {
            conn.execute(
                "INSERT INTO diff_analyses (project_id, from_commit, to_commit, analysis_type, files_json, created_at)
                 VALUES (?, 'abc', 'def', 'commit', NULL, datetime('now'))",
                rusqlite::params![project_id],
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .unwrap();

        // NULL files_json is filtered by the WHERE clause
        let result = get_fragile_modules(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_health_trend_single_snapshot() {
        use crate::db::test_support::setup_test_pool_with_project;
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Insert a single health snapshot (all NOT NULL columns required by schema)
        pool.run(move |conn| {
            conn.execute(
                "INSERT INTO health_snapshots
                 (project_id, avg_debt_score, max_debt_score, tier_distribution, module_count, snapshot_at)
                 VALUES (?, 42.0, 55.0, '{\"A\":5,\"B\":3}', 8, datetime('now'))",
                rusqlite::params![project_id],
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .unwrap();

        let result = get_health_trend(&pool, project_id).await;
        assert!(result.is_ok());
        let trend = result.unwrap();
        assert!(trend.is_some());
        let trend = trend.unwrap();
        assert!((trend.current_score - 42.0).abs() < 0.01);
        assert!(trend.previous_score.is_none());
        assert_eq!(trend.snapshot_count, 1);
        // No previous score -> direction is Stable
        assert!(matches!(
            trend.direction,
            super::super::types::TrendDirection::Stable
        ));
    }
}
