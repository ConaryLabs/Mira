//! MCP history logging
//!
//! Captures MCP tool calls for Claude Code sessions so they can be
//! recalled like chat history.

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::debug;

use crate::core::primitives::semantic::COLLECTION_CONVERSATION;
use crate::core::primitives::semantic_helpers::{store_with_logging, MetadataBuilder};
use crate::core::SemanticSearch;

/// Log an MCP tool call to history with optional semantic embedding
pub async fn log_call(
    db: &SqlitePool,
    session_id: Option<&str>,
    project_id: Option<i64>,
    tool_name: &str,
    arguments: Option<&serde_json::Value>,
    result_summary: &str,
    success: bool,
    duration_ms: Option<i64>,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();

    let args_json = arguments
        .map(|a| serde_json::to_string(a).unwrap_or_default());

    sqlx::query(
        r#"
        INSERT INTO mcp_history (id, session_id, project_id, tool_name, arguments, result_summary, success, duration_ms)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(session_id)
    .bind(project_id)
    .bind(tool_name)
    .bind(args_json)
    .bind(result_summary)
    .bind(if success { 1 } else { 0 })
    .bind(duration_ms)
    .execute(db)
    .await?;

    debug!(tool = tool_name, success, "MCP call logged to history");

    Ok(id)
}

/// Log an MCP tool call with semantic embedding for recall
pub async fn log_call_semantic(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    session_id: Option<&str>,
    project_id: Option<i64>,
    tool_name: &str,
    arguments: Option<&serde_json::Value>,
    result_summary: &str,
    success: bool,
    duration_ms: Option<i64>,
) -> Result<String> {
    // First log to DB
    let id = log_call(db, session_id, project_id, tool_name, arguments, result_summary, success, duration_ms).await?;

    // Then store semantic embedding
    let args_text = arguments
        .map(|a| serde_json::to_string(a).unwrap_or_default())
        .unwrap_or_default();

    // Build searchable content: tool name + args + summary
    let content = format!("{} {} {}", tool_name, args_text, result_summary);

    let metadata = MetadataBuilder::new("mcp_history")
        .string("tool_name", tool_name)
        .string("history_id", &id)
        .project_id(project_id)
        .bool("success", success)
        .build();

    store_with_logging(semantic, COLLECTION_CONVERSATION, &id, &content, metadata).await;

    Ok(id)
}

/// Search MCP history with optional filters
pub async fn search_history(
    db: &SqlitePool,
    project_id: Option<i64>,
    tool_name: Option<&str>,
    query: Option<&str>,
    limit: i64,
) -> Result<Vec<HistoryEntry>> {
    // Build query with all optional params as owned strings
    let pattern = query.map(|q| format!("%{}%", q));

    let results = match (project_id, tool_name, &pattern) {
        (Some(pid), Some(tn), Some(p)) => {
            sqlx::query_as::<_, HistoryEntry>(
                "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
                 FROM mcp_history WHERE project_id = ? AND tool_name = ? AND (result_summary LIKE ? OR arguments LIKE ?)
                 ORDER BY created_at DESC LIMIT ?"
            )
            .bind(pid).bind(tn).bind(p).bind(p).bind(limit)
            .fetch_all(db).await?
        }
        (Some(pid), Some(tn), None) => {
            sqlx::query_as::<_, HistoryEntry>(
                "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
                 FROM mcp_history WHERE project_id = ? AND tool_name = ?
                 ORDER BY created_at DESC LIMIT ?"
            )
            .bind(pid).bind(tn).bind(limit)
            .fetch_all(db).await?
        }
        (Some(pid), None, Some(p)) => {
            sqlx::query_as::<_, HistoryEntry>(
                "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
                 FROM mcp_history WHERE project_id = ? AND (result_summary LIKE ? OR arguments LIKE ?)
                 ORDER BY created_at DESC LIMIT ?"
            )
            .bind(pid).bind(p).bind(p).bind(limit)
            .fetch_all(db).await?
        }
        (Some(pid), None, None) => {
            sqlx::query_as::<_, HistoryEntry>(
                "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
                 FROM mcp_history WHERE project_id = ?
                 ORDER BY created_at DESC LIMIT ?"
            )
            .bind(pid).bind(limit)
            .fetch_all(db).await?
        }
        (None, Some(tn), Some(p)) => {
            sqlx::query_as::<_, HistoryEntry>(
                "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
                 FROM mcp_history WHERE tool_name = ? AND (result_summary LIKE ? OR arguments LIKE ?)
                 ORDER BY created_at DESC LIMIT ?"
            )
            .bind(tn).bind(p).bind(p).bind(limit)
            .fetch_all(db).await?
        }
        (None, Some(tn), None) => {
            sqlx::query_as::<_, HistoryEntry>(
                "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
                 FROM mcp_history WHERE tool_name = ?
                 ORDER BY created_at DESC LIMIT ?"
            )
            .bind(tn).bind(limit)
            .fetch_all(db).await?
        }
        (None, None, Some(p)) => {
            sqlx::query_as::<_, HistoryEntry>(
                "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
                 FROM mcp_history WHERE result_summary LIKE ? OR arguments LIKE ?
                 ORDER BY created_at DESC LIMIT ?"
            )
            .bind(p).bind(p).bind(limit)
            .fetch_all(db).await?
        }
        (None, None, None) => {
            sqlx::query_as::<_, HistoryEntry>(
                "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
                 FROM mcp_history ORDER BY created_at DESC LIMIT ?"
            )
            .bind(limit)
            .fetch_all(db).await?
        }
    };

    Ok(results)
}

/// Get recent MCP history for session context
pub async fn get_recent(
    db: &SqlitePool,
    project_id: Option<i64>,
    limit: i64,
) -> Result<Vec<HistoryEntry>> {
    let results = if let Some(pid) = project_id {
        sqlx::query_as::<_, HistoryEntry>(
            "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
             FROM mcp_history
             WHERE project_id = ?
             ORDER BY created_at DESC
             LIMIT ?"
        )
        .bind(pid)
        .bind(limit)
        .fetch_all(db)
        .await?
    } else {
        sqlx::query_as::<_, HistoryEntry>(
            "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
             FROM mcp_history
             ORDER BY created_at DESC
             LIMIT ?"
        )
        .bind(limit)
        .fetch_all(db)
        .await?
    };

    Ok(results)
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct HistoryEntry {
    pub id: String,
    pub session_id: Option<String>,
    pub tool_name: String,
    pub arguments: Option<String>,
    pub result_summary: Option<String>,
    pub success: i32,
    pub duration_ms: Option<i64>,
    pub created_at: String,
}

impl std::fmt::Display for HistoryEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.success == 1 { "✓" } else { "✗" };
        let summary = self.result_summary.as_deref().unwrap_or("(no summary)");
        let time = self.created_at.split('T').next().unwrap_or(&self.created_at);
        write!(f, "{} {} {} - {}", status, time, self.tool_name, summary)
    }
}

/// Semantic search of MCP history
pub async fn semantic_search(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Result<Vec<HistoryEntry>> {
    use crate::core::primitives::semantic_helpers::search_semantic;
    use qdrant_client::qdrant::{Condition, Filter};

    // Build filter for mcp_history type and optional project
    let mut conditions = vec![
        Condition::matches("type", "mcp_history".to_string()),
    ];
    if let Some(pid) = project_id {
        conditions.push(Condition::matches("project_id", pid));
    }
    let filter = Filter::must(conditions);

    // Search semantically
    let results = search_semantic(semantic, COLLECTION_CONVERSATION, query, limit, Some(filter)).await;

    if let Some(results) = results {
        // Extract history IDs and fetch full entries from DB
        let ids: Vec<String> = results
            .iter()
            .filter_map(|r| r.metadata.get("history_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();

        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Fetch entries by IDs (preserving order)
        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let query_str = format!(
            "SELECT id, session_id, tool_name, arguments, result_summary, success, duration_ms, created_at
             FROM mcp_history WHERE id IN ({}) ORDER BY created_at DESC",
            placeholders
        );

        let mut q = sqlx::query_as::<_, HistoryEntry>(&query_str);
        for id in &ids {
            q = q.bind(id);
        }

        let entries = q.fetch_all(db).await?;
        Ok(entries)
    } else {
        // Fallback to simple SQL search
        search_history(db, project_id, None, Some(query), limit as i64).await
    }
}
