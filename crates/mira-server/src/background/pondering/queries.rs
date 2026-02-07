// background/pondering/queries.rs
// Database queries for pondering data gathering

use super::types::{
    FragileModule, MemoryEntry, ProjectInsightData, RevertCluster, SessionPattern, StaleGoal,
    ToolUsageEntry, UntestedFile,
};
use crate::db::pool::DatabasePool;
use crate::utils::{ResultExt, truncate};
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
    pool.interact(move |conn| {
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
    .str_err()
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

/// Get recent memories for a project
pub(super) async fn get_recent_memories(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<MemoryEntry>, String> {
    pool.interact(move |conn| {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT content, fact_type, category, status
                FROM memory_facts
                WHERE project_id = ?
                  AND updated_at > datetime('now', '-7 days')
                ORDER BY updated_at DESC
                LIMIT 50
            "#,
            )
            .map_err(|e| anyhow::anyhow!("Failed to prepare: {}", e))?;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(MemoryEntry {
                    content: row.get(0)?,
                    fact_type: row.get(1)?,
                    category: row.get(2)?,
                    status: row.get(3)?,
                })
            })
            .map_err(|e| anyhow::anyhow!("Failed to query: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Failed to collect: {}", e))
    })
    .await
    .str_err()
}

// ── New project-aware queries ──────────────────────────────────────────

/// Goals with `status = 'in_progress'` that haven't been updated recently.
/// Uses `created_at` since the goals table has no `updated_at` column.
pub(super) async fn get_stale_goals(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<StaleGoal>, String> {
    pool.interact(move |conn| {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT
                    g.id,
                    g.title,
                    g.status,
                    g.progress_percent,
                    CAST(julianday('now') - julianday(g.created_at) AS INTEGER) AS days_since_update,
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
                  AND g.created_at < datetime('now', '-14 days')
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
    .str_err()
}

/// Modules where a significant portion of diffs resulted in reverts or follow-up fixes.
/// Groups by top-level directory extracted from `files_json`.
pub(super) async fn get_fragile_modules(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<FragileModule>, String> {
    pool.interact(move |conn| {
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

        Ok(results)
    })
    .await
    .str_err()
}

/// 2+ reverts in the same module within 48h, looking back 7 days.
pub(super) async fn get_recent_revert_clusters(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<RevertCluster>, String> {
    pool.interact(move |conn| {
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
        Ok(clusters)
    })
    .await
    .str_err()
}

/// Files modified 5+ times across multiple sessions without corresponding test file changes.
/// Uses session_behavior_log with event_type = 'file_access' and event_data containing file paths.
pub(super) async fn get_untested_hotspots(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<UntestedFile>, String> {
    pool.interact(move |conn| {
        // Get file modification events from session_behavior_log
        let mut stmt = conn
            .prepare(
                r#"
                SELECT
                    sbl.event_data,
                    sbl.session_id
                FROM session_behavior_log sbl
                WHERE sbl.project_id = ?
                  AND sbl.event_type = 'file_access'
                  AND sbl.created_at > datetime('now', '-30 days')
                ORDER BY sbl.created_at DESC
            "#,
            )
            .map_err(|e| anyhow::anyhow!("Failed to prepare untested hotspots: {}", e))?;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| anyhow::anyhow!("Failed to query untested hotspots: {}", e))?;

        // Track per-file: (modification_count, set of sessions)
        let mut file_stats: HashMap<String, (i64, std::collections::HashSet<String>)> =
            HashMap::new();
        let mut test_files_modified: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for row in rows {
            let (event_data, session_id) =
                row.map_err(|e| anyhow::anyhow!("Row error: {}", e))?;

            // event_data is JSON, extract file_path
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&event_data) {
                if let Some(file_path) = data
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                {
                    // Track test files separately
                    if is_test_file(&file_path) {
                        test_files_modified.insert(file_path);
                        continue;
                    }

                    let entry = file_stats.entry(file_path).or_insert((0, Default::default()));
                    entry.0 += 1;
                    entry.1.insert(session_id);
                }
            }
        }

        // Filter: 5+ modifications, multiple sessions, no corresponding test file modified
        let mut results: Vec<UntestedFile> = file_stats
            .into_iter()
            .filter(|(path, (count, sessions))| {
                *count >= 5
                    && sessions.len() >= 2
                    && !has_corresponding_test(&test_files_modified, path)
            })
            .map(|(file_path, (count, sessions))| UntestedFile {
                file_path,
                modification_count: count,
                sessions_involved: sessions.len() as i64,
            })
            .collect();

        results.sort_by(|a, b| b.modification_count.cmp(&a.modification_count));
        Ok(results)
    })
    .await
    .str_err()
}

/// Detect session-level patterns: many short sessions, sessions with no commits, long gaps.
pub(super) async fn get_session_patterns(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<SessionPattern>, String> {
    pool.interact(move |conn| {
        let mut patterns = Vec::new();

        // Pattern 1: Short sessions (< 5 minutes)
        let short_sessions: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM sessions
                WHERE project_id = ?
                  AND started_at > datetime('now', '-7 days')
                  AND last_activity IS NOT NULL
                  AND (julianday(last_activity) - julianday(started_at)) * 24 * 60 < 5
            "#,
                params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if short_sessions >= 3 {
            patterns.push(SessionPattern {
                description: format!(
                    "{} sessions in the last 7 days lasted less than 5 minutes",
                    short_sessions
                ),
                count: short_sessions,
            });
        }

        // Pattern 2: Total sessions in last 7 days (high churn indicator)
        let total_sessions: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM sessions
                WHERE project_id = ?
                  AND started_at > datetime('now', '-7 days')
            "#,
                params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if total_sessions >= 10 {
            patterns.push(SessionPattern {
                description: format!(
                    "{} sessions in the last 7 days — high context-switching frequency",
                    total_sessions
                ),
                count: total_sessions,
            });
        }

        // Pattern 3: Sessions without summaries (may indicate incomplete work)
        let no_summary_sessions: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM sessions
                WHERE project_id = ?
                  AND started_at > datetime('now', '-7 days')
                  AND (summary IS NULL OR summary = '')
            "#,
                params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if no_summary_sessions >= 3 {
            patterns.push(SessionPattern {
                description: format!(
                    "{} sessions in the last 7 days ended without a summary",
                    no_summary_sessions
                ),
                count: no_summary_sessions,
            });
        }

        Ok(patterns)
    })
    .await
    .str_err()
}

/// Gather all project insight data in one call.
pub(super) async fn get_project_insight_data(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<ProjectInsightData, String> {
    // Run all queries concurrently
    let (stale_goals, fragile_modules, revert_clusters, untested_hotspots, session_patterns) =
        tokio::join!(
            get_stale_goals(pool, project_id),
            get_fragile_modules(pool, project_id),
            get_recent_revert_clusters(pool, project_id),
            get_untested_hotspots(pool, project_id),
            get_session_patterns(pool, project_id),
        );

    Ok(ProjectInsightData {
        stale_goals: stale_goals.unwrap_or_default(),
        fragile_modules: fragile_modules.unwrap_or_default(),
        revert_clusters: revert_clusters.unwrap_or_default(),
        untested_hotspots: untested_hotspots.unwrap_or_default(),
        session_patterns: session_patterns.unwrap_or_default(),
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

/// Check if a file path looks like a test file.
fn is_test_file(path: &str) -> bool {
    path.contains("test")
        || path.contains("spec")
        || path.ends_with("_test.rs")
        || path.ends_with("_test.go")
        || path.ends_with(".test.ts")
        || path.ends_with(".test.js")
        || path.ends_with("_spec.rb")
}

/// Check if a source file has a corresponding test file in the modified set.
fn has_corresponding_test(
    test_files: &std::collections::HashSet<String>,
    source_path: &str,
) -> bool {
    // Check for common test file naming patterns
    let stem = source_path
        .trim_end_matches(".rs")
        .trim_end_matches(".ts")
        .trim_end_matches(".js")
        .trim_end_matches(".py")
        .trim_end_matches(".go")
        .trim_end_matches(".rb");

    let test_variants = [
        format!("{}_test.rs", stem),
        format!("{}.test.ts", stem),
        format!("{}.test.js", stem),
        format!("test_{}.py", stem),
        format!("{}_test.go", stem),
        format!("{}_spec.rb", stem),
    ];

    for variant in &test_variants {
        if test_files.contains(variant) {
            return true;
        }
    }

    // Also check if any test file is in the same directory
    if let Some(dir) = source_path.rsplit_once('/').map(|(d, _)| d) {
        for test_file in test_files {
            if test_file.starts_with(dir) && is_test_file(test_file) {
                return true;
            }
        }
    }

    false
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

    #[test]
    fn test_is_test_file() {
        assert!(is_test_file("src/db/pool_test.rs"));
        assert!(is_test_file("src/components/Button.test.ts"));
        assert!(is_test_file("tests/integration.rs"));
        assert!(!is_test_file("src/db/pool.rs"));
        assert!(!is_test_file("src/main.rs"));
    }

    #[test]
    fn test_has_corresponding_test() {
        let mut test_files = std::collections::HashSet::new();
        test_files.insert("src/db/pool_test.rs".to_string());

        // Direct name match
        assert!(has_corresponding_test(&test_files, "src/db/pool.rs"));

        // Different directory — no match
        assert!(!has_corresponding_test(
            &test_files,
            "src/background/worker.rs"
        ));

        // Same directory counts as tested (test file exists in same dir)
        assert!(has_corresponding_test(&test_files, "src/db/tasks.rs"));
    }
}
