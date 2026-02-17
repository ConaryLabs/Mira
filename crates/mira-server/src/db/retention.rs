//! Data retention — periodic cleanup for unbounded tables.
//!
//! Retention policy (configurable via `[retention]` in config.toml):
//! - tool_history_days (default 30): tool_history,
//!   session_behavior_log, proactive_interventions, injection_feedback, proactive_suggestions
//! - sessions_days (default 90): sessions (completed only), session_snapshots, session_tasks,
//!   session_goals
//! - analytics_days (default 180): llm_usage, embeddings_usage
//! - behavior_days (default 365): behavior_patterns (non-insight)
//! - observations_days (default 90): system_observations
//!
//! Retention rules only run when explicitly enabled. Orphan cleanup always runs.

use crate::config::file::RetentionConfig;
use rusqlite::Connection;

/// Table name + column used for age comparison + retention days.
struct RetentionRule {
    table: &'static str,
    time_column: &'static str,
    days: u32,
    /// Optional extra WHERE clause (e.g. "AND status = 'completed'")
    extra_filter: &'static str,
}

/// Build retention rules from config. Ordering matters for FK constraints:
/// child tables are listed before their parents.
fn build_rules(config: &RetentionConfig) -> Vec<RetentionRule> {
    vec![
        // ── Children of sessions (must delete before sessions) ──
        RetentionRule {
            table: "session_snapshots",
            time_column: "created_at",
            days: config.sessions_days,
            extra_filter: "",
        },
        RetentionRule {
            table: "session_goals",
            time_column: "created_at",
            days: config.sessions_days,
            extra_filter: "",
        },
        RetentionRule {
            table: "session_tasks",
            time_column: "updated_at",
            days: config.sessions_days,
            extra_filter: "AND status IN ('completed', 'deleted')",
        },
        RetentionRule {
            table: "tool_history",
            time_column: "created_at",
            days: config.tool_history_days,
            extra_filter: "AND session_id NOT IN (SELECT id FROM sessions WHERE status = 'active')",
        },
        // ── Children of diff_analyses (must delete before parent) ──
        RetentionRule {
            table: "diff_outcomes",
            time_column: "created_at",
            days: config.tool_history_days,
            extra_filter: "",
        },
        RetentionRule {
            table: "diff_analyses",
            time_column: "created_at",
            days: config.tool_history_days,
            extra_filter: "",
        },
        // ── Tool-history-cadence tables ──
        RetentionRule {
            table: "session_behavior_log",
            time_column: "created_at",
            days: config.tool_history_days,
            extra_filter: "",
        },
        RetentionRule {
            table: "proactive_interventions",
            time_column: "created_at",
            days: config.tool_history_days,
            extra_filter: "",
        },
        RetentionRule {
            table: "injection_feedback",
            time_column: "created_at",
            days: config.tool_history_days,
            extra_filter: "",
        },
        RetentionRule {
            table: "proactive_suggestions",
            time_column: "created_at",
            days: config.tool_history_days,
            extra_filter: "",
        },
        // ── Analytics ──
        RetentionRule {
            table: "llm_usage",
            time_column: "created_at",
            days: config.analytics_days,
            extra_filter: "",
        },
        RetentionRule {
            table: "embeddings_usage",
            time_column: "created_at",
            days: config.analytics_days,
            extra_filter: "",
        },
        // ── Sessions (parent — after children above) ──
        RetentionRule {
            table: "sessions",
            time_column: "last_activity",
            days: config.sessions_days,
            extra_filter: "AND status = 'completed'",
        },
        // ── Error patterns ──
        RetentionRule {
            table: "error_patterns",
            time_column: "updated_at",
            days: config.behavior_days,
            extra_filter: "",
        },
        // ── Behavior patterns ──
        RetentionRule {
            table: "behavior_patterns",
            time_column: "created_at",
            days: config.behavior_days,
            extra_filter: "AND pattern_type NOT LIKE 'insight_%'",
        },
        // ── Observations (hard cutoff beyond per-row TTL) ──
        RetentionRule {
            table: "system_observations",
            time_column: "created_at",
            days: config.observations_days,
            extra_filter: "",
        },
        // ── Health snapshots (same cadence as observations) ──
        RetentionRule {
            table: "health_snapshots",
            time_column: "snapshot_at",
            days: config.observations_days,
            extra_filter: "",
        },
    ]
}

/// Run data retention for all rules. Returns total deleted rows.
/// Designed to be called from `pool.run()` via a closure that captures config.
pub fn run_data_retention_sync(
    conn: &Connection,
    config: &RetentionConfig,
) -> Result<usize, String> {
    let rules = build_rules(config);
    let mut total_deleted = 0;

    for rule in &rules {
        // Batched deletes: use a subquery with LIMIT to avoid holding SQLite's
        // write lock for large backlogs. The subquery approach works without
        // SQLITE_ENABLE_UPDATE_DELETE_LIMIT.
        let sql = format!(
            "DELETE FROM {table} WHERE rowid IN \
             (SELECT rowid FROM {table} WHERE {col} < datetime('now', '-{days} days') {extra} LIMIT 10000)",
            table = rule.table,
            col = rule.time_column,
            days = rule.days,
            extra = rule.extra_filter,
        );

        loop {
            match conn.execute(&sql, []) {
                Ok(0) => break,
                Ok(count) => {
                    total_deleted += count;
                    tracing::info!(
                        "[retention] Deleted {} rows from {} (>{} days old, batch)",
                        count,
                        rule.table,
                        rule.days
                    );
                    // If we deleted fewer than the batch limit, we're done
                    if count < 10_000 {
                        break;
                    }
                }
                Err(e) => {
                    // Table might not exist yet (migrations not applied) — log and continue
                    tracing::warn!("[retention] Failed to clean {}: {}", rule.table, e);
                    break;
                }
            }
        }
    }

    Ok(total_deleted)
}

/// Execute a SQL statement, returning 0 on failure (table might not exist).
fn try_execute(conn: &Connection, sql: &str) -> usize {
    match conn.execute(sql, []) {
        Ok(count) => count,
        Err(e) => {
            tracing::debug!("[retention] orphan cleanup query failed (may be OK): {}", e);
            0
        }
    }
}

/// Clean up orphaned rows that reference deleted parents.
/// Always runs regardless of retention enabled/disabled (data integrity, not policy).
pub fn cleanup_orphans(conn: &Connection) -> Result<usize, String> {
    let mut total = 0;

    // vec_memory orphans (virtual table, no FK cascade possible)
    total += try_execute(
        conn,
        "DELETE FROM vec_memory WHERE fact_id NOT IN (SELECT id FROM memory_facts)",
    );
    // session_snapshots without parent session
    total += try_execute(
        conn,
        "DELETE FROM session_snapshots WHERE session_id NOT IN (SELECT id FROM sessions)",
    );
    // tool_history without parent session
    total += try_execute(
        conn,
        "DELETE FROM tool_history WHERE session_id IS NOT NULL AND session_id NOT IN (SELECT id FROM sessions)",
    );
    // session_goals without parent session
    total += try_execute(
        conn,
        "DELETE FROM session_goals WHERE session_id NOT IN (SELECT id FROM sessions)",
    );
    // session_goals without parent goal
    total += try_execute(
        conn,
        "DELETE FROM session_goals WHERE goal_id NOT IN (SELECT id FROM goals)",
    );
    // error_patterns without parent project
    total += try_execute(
        conn,
        "DELETE FROM error_patterns WHERE project_id NOT IN (SELECT id FROM projects)",
    );
    // orphaned memory_entities (no links remaining)
    total += try_execute(
        conn,
        "DELETE FROM memory_entities WHERE id NOT IN (SELECT DISTINCT entity_id FROM memory_entity_links)",
    );

    // Reclaim space if we deleted a lot
    if total > 1000 {
        let _ = conn.execute_batch("PRAGMA incremental_vacuum(100);");
    }

    if total > 0 {
        tracing::info!("[retention] Cleaned up {} orphaned rows", total);
    }

    Ok(total)
}

/// Dry-run: count how many rows each retention rule would delete.
/// Returns vec of (table_name, candidate_count) pairs.
pub fn count_retention_candidates(
    conn: &Connection,
    config: &RetentionConfig,
) -> Vec<(String, usize)> {
    let rules = build_rules(config);
    let mut results = Vec::new();

    for rule in &rules {
        let sql = format!(
            "SELECT COUNT(*) FROM {table} WHERE {col} < datetime('now', '-{days} days') {extra}",
            table = rule.table,
            col = rule.time_column,
            days = rule.days,
            extra = rule.extra_filter,
        );

        let count: usize = conn.query_row(&sql, [], |row| row.get(0)).unwrap_or(0);

        if count > 0 {
            results.push((rule.table.to_string(), count));
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Set up a minimal in-memory DB with just the tables cleanup_orphans touches.
    fn setup_orphan_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE sessions (id TEXT PRIMARY KEY, created_at TEXT);
            CREATE TABLE memory_facts (id INTEGER PRIMARY KEY);
            CREATE TABLE vec_memory (fact_id INTEGER);
            CREATE TABLE session_snapshots (id INTEGER PRIMARY KEY, session_id TEXT);
            CREATE TABLE tool_history (id INTEGER PRIMARY KEY, session_id TEXT, created_at TEXT);
            CREATE TABLE memory_entities (id INTEGER PRIMARY KEY);
            CREATE TABLE memory_entity_links (entity_id INTEGER);
            ",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_cleanup_orphans_no_false_positives() {
        let conn = setup_orphan_test_db();

        // Insert a session and linked rows across multiple tables
        conn.execute("INSERT INTO sessions (id) VALUES ('s1')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO session_snapshots (session_id) VALUES ('s1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tool_history (session_id, created_at) VALUES ('s1', datetime('now'))",
            [],
        )
        .unwrap();

        let cleaned = cleanup_orphans(&conn).unwrap();
        assert_eq!(
            cleaned, 0,
            "nothing should be cleaned when all rows have valid parents"
        );
    }
}
