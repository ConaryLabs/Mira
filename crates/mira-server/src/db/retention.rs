//! Data retention — periodic cleanup for unbounded tables.
//!
//! Retention policy:
//! - 30 days: tool_history, chat_messages, chat_summaries, session_behavior_log, proactive_interventions
//! - 60 days: diff_analyses, diff_outcomes, pattern_sharing_log
//! - 90 days: llm_usage, embeddings_usage, sessions (completed only), session_snapshots

use rusqlite::Connection;

/// Table name + column used for age comparison + retention days.
struct RetentionRule {
    table: &'static str,
    time_column: &'static str,
    days: u32,
    /// Optional extra WHERE clause (e.g. "AND status = 'completed'")
    extra_filter: &'static str,
}

const RULES: &[RetentionRule] = &[
    // 30-day retention
    RetentionRule {
        table: "tool_history",
        time_column: "created_at",
        days: 30,
        extra_filter: "",
    },
    RetentionRule {
        table: "chat_messages",
        time_column: "created_at",
        days: 30,
        extra_filter: "",
    },
    RetentionRule {
        table: "chat_summaries",
        time_column: "created_at",
        days: 30,
        extra_filter: "",
    },
    RetentionRule {
        table: "session_behavior_log",
        time_column: "created_at",
        days: 30,
        extra_filter: "",
    },
    RetentionRule {
        table: "proactive_interventions",
        time_column: "created_at",
        days: 30,
        extra_filter: "",
    },
    // 60-day retention
    RetentionRule {
        table: "diff_analyses",
        time_column: "created_at",
        days: 60,
        extra_filter: "",
    },
    RetentionRule {
        table: "diff_outcomes",
        time_column: "created_at",
        days: 60,
        extra_filter: "",
    },
    RetentionRule {
        table: "pattern_sharing_log",
        time_column: "created_at",
        days: 60,
        extra_filter: "",
    },
    // 90-day retention
    RetentionRule {
        table: "llm_usage",
        time_column: "created_at",
        days: 90,
        extra_filter: "",
    },
    RetentionRule {
        table: "embeddings_usage",
        time_column: "created_at",
        days: 90,
        extra_filter: "",
    },
    RetentionRule {
        table: "sessions",
        time_column: "last_activity",
        days: 90,
        extra_filter: "AND status = 'completed'",
    },
    RetentionRule {
        table: "session_snapshots",
        time_column: "created_at",
        days: 90,
        extra_filter: "",
    },
];

/// Run data retention for all rules. Returns total deleted rows.
/// Designed to be called from `pool.interact()`.
pub fn run_data_retention_sync(conn: &Connection) -> Result<usize, String> {
    let mut total_deleted = 0;

    for rule in RULES {
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
