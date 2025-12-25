//! Message loading and semantic recall

use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::sync::Arc;

use crate::core::SemanticSearch;
use super::types::{ChatMessage, SemanticHit};

/// Collection name for chat messages
pub const COLLECTION_CHAT: &str = "mira_chat_messages";

/// Minimum similarity score for semantic recall
pub const RECALL_THRESHOLD: f32 = 0.75;

/// Number of semantic results to fetch
pub const RECALL_LIMIT: usize = 3;

/// Load recent messages from the database
pub async fn load_recent_messages(db: &SqlitePool, limit: usize) -> Result<Vec<ChatMessage>> {
    let rows = sqlx::query(
        r#"
        SELECT id, role, blocks, created_at
        FROM chat_messages
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit as i64)
    .fetch_all(db)
    .await?;

    let mut messages: Vec<ChatMessage> = rows
        .into_iter()
        .filter_map(|row| {
            let id: String = row.get("id");
            let role: String = row.get("role");
            let blocks_json: String = row.get("blocks");
            let created_at: i64 = row.get("created_at");

            // Extract text content from blocks
            let blocks: Vec<serde_json::Value> = serde_json::from_str(&blocks_json).ok()?;
            let content = blocks
                .iter()
                .filter_map(|b| b.get("content")?.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            Some(ChatMessage {
                id,
                role,
                content,
                created_at,
            })
        })
        .collect();

    // Reverse to get chronological order
    messages.reverse();
    Ok(messages)
}

/// Semantic recall of relevant past CONVERSATION context (not code!)
/// Scoped to current project
pub async fn semantic_recall(
    semantic: &Arc<SemanticSearch>,
    project_path: &str,
    query: &str,
) -> Result<Vec<SemanticHit>> {
    use qdrant_client::qdrant::{Condition, Filter};

    // Filter to only this project's messages
    let filter = Filter::must([Condition::matches("project", project_path.to_string())]);

    let results = semantic
        .search(COLLECTION_CHAT, query, RECALL_LIMIT, Some(filter))
        .await?;

    Ok(results
        .into_iter()
        .filter(|r| r.score >= RECALL_THRESHOLD)
        .map(|r| SemanticHit {
            content: r.content,
            score: r.score,
            role: r
                .metadata
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            created_at: r.metadata.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0),
        })
        .collect())
}
