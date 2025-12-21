//! Core chat summarization operations - shared by Chat/Studio
//!
//! Rolling summaries and meta-summarization for context compression.
//! Used by the SessionManager in Chat mode.

use chrono::Utc;
use uuid::Uuid;

use super::super::{CoreResult, OpContext};

// ============================================================================
// Input/Output Types
// ============================================================================

/// A chat message for summarization
#[derive(Debug, Clone)]
pub struct ChatMessageForSummary {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

/// Summary info for meta-summarization
#[derive(Debug, Clone)]
pub struct SummaryForMeta {
    pub id: String,
    pub summary: String,
}

/// Result of storing a summary
#[derive(Debug, Clone)]
pub struct StoreSummaryOutput {
    pub summary_id: String,
    pub archived_count: usize,
}

// ============================================================================
// Operations
// ============================================================================

/// Load rolling summaries with tiered support
/// Prioritizes meta-summaries (level 2) over regular summaries (level 1)
pub async fn load_summaries(
    ctx: &OpContext,
    project_path: &str,
    limit: usize,
) -> CoreResult<Vec<String>> {
    let db = ctx.require_db()?;

    // First get any meta-summaries (level 2)
    let meta: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT summary FROM chat_summaries
        WHERE project_path = $1 AND level = 2
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(project_path)
    .fetch_all(db)
    .await?;

    let mut summaries: Vec<String> = meta.into_iter().map(|(s,)| s).collect();
    let remaining = limit.saturating_sub(summaries.len());

    // Then get recent level-1 summaries
    if remaining > 0 {
        let recent: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT summary FROM chat_summaries
            WHERE project_path = $1 AND level = 1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(project_path)
        .bind(remaining as i64)
        .fetch_all(db)
        .await?;

        summaries.extend(recent.into_iter().map(|(s,)| s));
    }

    Ok(summaries)
}

/// Check if meta-summarization is needed (too many level-1 summaries)
/// Returns summaries to compress if threshold exceeded
pub async fn get_summaries_for_meta(
    ctx: &OpContext,
    project_path: &str,
    threshold: usize,
) -> CoreResult<Option<Vec<SummaryForMeta>>> {
    let db = ctx.require_db()?;

    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM chat_summaries WHERE project_path = $1 AND level = 1",
    )
    .bind(project_path)
    .fetch_one(db)
    .await?;

    if (count.0 as usize) < threshold {
        return Ok(None);
    }

    // Get oldest level-1 summaries to compress
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT id, summary FROM chat_summaries
        WHERE project_path = $1 AND level = 1
        ORDER BY created_at ASC
        LIMIT $2
        "#,
    )
    .bind(project_path)
    .bind(threshold as i64)
    .fetch_all(db)
    .await?;

    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            rows.into_iter()
                .map(|(id, summary)| SummaryForMeta { id, summary })
                .collect(),
        ))
    }
}

/// Store a meta-summary (level 2) and delete the summarized level-1 summaries
pub async fn store_meta_summary(
    ctx: &OpContext,
    project_path: &str,
    summary: &str,
    summary_ids: &[String],
) -> CoreResult<String> {
    let db = ctx.require_db()?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    // Store the meta-summary
    sqlx::query(
        r#"
        INSERT INTO chat_summaries (id, project_path, summary, message_ids, message_count, level, created_at)
        VALUES ($1, $2, $3, $4, $5, 2, $6)
        "#,
    )
    .bind(&id)
    .bind(project_path)
    .bind(summary)
    .bind(serde_json::to_string(summary_ids).unwrap_or_else(|_| "[]".to_string()))
    .bind(summary_ids.len() as i64)
    .bind(now)
    .execute(db)
    .await?;

    // Delete the old level-1 summaries
    for sum_id in summary_ids {
        sqlx::query("DELETE FROM chat_summaries WHERE id = $1")
            .bind(sum_id)
            .execute(db)
            .await?;
    }

    Ok(id)
}

/// Check if summarization is needed
/// Returns messages to summarize if threshold exceeded
pub async fn get_messages_for_summary(
    ctx: &OpContext,
    threshold: usize,
    recent_raw_count: usize,
    batch_size: usize,
) -> CoreResult<Option<Vec<ChatMessageForSummary>>> {
    let db = ctx.require_db()?;

    // Count total messages
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM chat_messages")
        .fetch_one(db)
        .await?;

    if count.0 as usize <= threshold {
        return Ok(None);
    }

    // Get oldest messages outside the recent window (to be summarized)
    let to_summarize_count = count.0 as usize - recent_raw_count;
    if to_summarize_count < batch_size {
        return Ok(None);
    }

    // Fetch the oldest messages that will be summarized
    let rows = sqlx::query(
        r#"
        SELECT id, role, blocks, created_at
        FROM chat_messages
        ORDER BY created_at ASC
        LIMIT $1
        "#,
    )
    .bind(to_summarize_count as i64)
    .fetch_all(db)
    .await?;

    use sqlx::Row;
    let messages: Vec<ChatMessageForSummary> = rows
        .into_iter()
        .filter_map(|row| {
            let id: String = row.get("id");
            let role: String = row.get("role");
            let blocks_json: String = row.get("blocks");
            let created_at: i64 = row.get("created_at");

            let blocks: Vec<serde_json::Value> = serde_json::from_str(&blocks_json).ok()?;
            let content = blocks
                .iter()
                .filter_map(|b| b.get("content")?.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            Some(ChatMessageForSummary {
                id,
                role,
                content,
                created_at,
            })
        })
        .collect();

    if messages.is_empty() {
        Ok(None)
    } else {
        Ok(Some(messages))
    }
}

/// Store a summary and archive the summarized messages
pub async fn store_summary(
    ctx: &OpContext,
    project_path: &str,
    summary: &str,
    message_ids: &[String],
) -> CoreResult<StoreSummaryOutput> {
    let db = ctx.require_db()?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    // Store the summary
    sqlx::query(
        r#"
        INSERT INTO chat_summaries (id, project_path, summary, message_ids, message_count, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(&id)
    .bind(project_path)
    .bind(summary)
    .bind(serde_json::to_string(message_ids).unwrap_or_else(|_| "[]".to_string()))
    .bind(message_ids.len() as i64)
    .bind(now)
    .execute(db)
    .await?;

    // Archive the old messages (don't delete - preserve for recall)
    for msg_id in message_ids {
        sqlx::query(
            "UPDATE chat_messages SET archived_at = $1, summary_id = $2 WHERE id = $3",
        )
        .bind(now)
        .bind(&id)
        .bind(msg_id)
        .execute(db)
        .await?;
    }

    // Update message count (only active, non-archived)
    let remaining: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM chat_messages WHERE archived_at IS NULL",
    )
    .fetch_one(db)
    .await?;

    sqlx::query(
        "UPDATE chat_context SET total_messages = $1, updated_at = $2 WHERE project_path = $3",
    )
    .bind(remaining.0)
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(StoreSummaryOutput {
        summary_id: id,
        archived_count: message_ids.len(),
    })
}

/// Store a per-turn summary (doesn't delete messages - just adds summary)
/// Used for immediate turn summarization in fresh-chain-per-turn mode
pub async fn store_turn_summary(
    ctx: &OpContext,
    project_path: &str,
    summary: &str,
) -> CoreResult<String> {
    let db = ctx.require_db()?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    sqlx::query(
        r#"
        INSERT INTO chat_summaries (id, project_path, summary, message_ids, message_count, level, created_at)
        VALUES ($1, $2, $3, '[]', 1, 1, $4)
        "#,
    )
    .bind(&id)
    .bind(project_path)
    .bind(summary)
    .bind(now)
    .execute(db)
    .await?;

    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_for_summary_fields() {
        let msg = ChatMessageForSummary {
            id: "msg-123".to_string(),
            role: "assistant".to_string(),
            content: "Hello, how can I help?".to_string(),
            created_at: 1700000000,
        };
        assert_eq!(msg.id, "msg-123");
        assert_eq!(msg.role, "assistant");
        assert!(msg.content.contains("Hello"));
        assert_eq!(msg.created_at, 1700000000);
    }

    #[test]
    fn test_summary_for_meta_fields() {
        let sum = SummaryForMeta {
            id: "sum-456".to_string(),
            summary: "User asked about Rust. Assistant explained ownership.".to_string(),
        };
        assert_eq!(sum.id, "sum-456");
        assert!(sum.summary.contains("Rust"));
    }

    #[test]
    fn test_store_summary_output_fields() {
        let output = StoreSummaryOutput {
            summary_id: "sum-789".to_string(),
            archived_count: 15,
        };
        assert_eq!(output.summary_id, "sum-789");
        assert_eq!(output.archived_count, 15);
    }

    #[test]
    fn test_threshold_check_logic() {
        // Simulates threshold check: if count <= threshold, don't summarize
        let threshold = 20;
        let count = 15;
        assert!(count <= threshold); // Should NOT trigger summarization

        let count2 = 25;
        assert!(count2 > threshold); // SHOULD trigger summarization
    }

    #[test]
    fn test_batch_size_check() {
        // If to_summarize < batch_size, don't summarize
        let total_count = 25;
        let recent_raw_count = 10;
        let batch_size = 20;

        let to_summarize = total_count - recent_raw_count;
        assert_eq!(to_summarize, 15);
        assert!(to_summarize < batch_size); // Not enough to batch

        let total_count2 = 40;
        let to_summarize2 = total_count2 - recent_raw_count;
        assert_eq!(to_summarize2, 30);
        assert!(to_summarize2 >= batch_size); // Enough to batch
    }

    #[test]
    fn test_content_extraction_from_blocks() {
        // Simulates extracting content from message blocks
        let blocks_json = r#"[{"type":"text","content":"Hello"},{"type":"text","content":"World"}]"#;
        let blocks: Vec<serde_json::Value> = serde_json::from_str(blocks_json).unwrap();

        let content: String = blocks
            .iter()
            .filter_map(|b| b.get("content")?.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(content, "Hello\nWorld");
    }
}
