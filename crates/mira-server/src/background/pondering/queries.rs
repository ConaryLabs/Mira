// background/pondering/queries.rs
// Database queries for pondering data gathering

use super::types::{MemoryEntry, ToolUsageEntry};
use crate::db::pool::DatabasePool;
use crate::utils::ResultExt;
use rusqlite::params;
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
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(args) {
        if let Some(obj) = value.as_object() {
            let keys: Vec<&str> = obj.keys().map(|s| s.as_str()).collect();
            return format!("keys: {}", keys.join(", "));
        }
    }
    // Fallback: truncate
    if args.len() > 50 {
        format!("{}...", &args[..50])
    } else {
        args.to_string()
    }
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
}
