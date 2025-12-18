//! Message summarization
//!
//! Handles rolling summaries and meta-summarization for context compression.

use anyhow::Result;
use chrono::Utc;
use tracing::{debug, info};
use uuid::Uuid;

use super::{ChatMessage, SessionManager, SUMMARIZE_BATCH_SIZE, SUMMARIZE_THRESHOLD, RECENT_RAW_COUNT, META_SUMMARY_THRESHOLD};

impl SessionManager {
    /// Load rolling summaries with tiered support
    /// Prioritizes meta-summaries (level 2) over regular summaries (level 1)
    pub(super) async fn load_summaries(&self, limit: usize) -> Result<Vec<String>> {
        // First get any meta-summaries (level 2)
        let meta: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT summary FROM chat_summaries
            WHERE project_path = $1 AND level = 2
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(&self.project_path)
        .fetch_all(&self.db)
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
            .bind(&self.project_path)
            .bind(remaining as i64)
            .fetch_all(&self.db)
            .await?;

            summaries.extend(recent.into_iter().map(|(s,)| s));
        }

        Ok(summaries)
    }

    /// Check if meta-summarization is needed (too many level-1 summaries)
    pub async fn check_meta_summarization_needed(&self) -> Result<Option<Vec<(String, String)>>> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM chat_summaries WHERE project_path = $1 AND level = 1",
        )
        .bind(&self.project_path)
        .fetch_one(&self.db)
        .await?;

        if (count.0 as usize) < META_SUMMARY_THRESHOLD {
            return Ok(None);
        }

        info!(
            "Meta-summarization needed: {} level-1 summaries to compress",
            count.0
        );

        // Get oldest level-1 summaries to compress
        let rows: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT id, summary FROM chat_summaries
            WHERE project_path = $1 AND level = 1
            ORDER BY created_at ASC
            LIMIT $2
            "#,
        )
        .bind(&self.project_path)
        .bind(META_SUMMARY_THRESHOLD as i64)
        .fetch_all(&self.db)
        .await?;

        if rows.is_empty() {
            Ok(None)
        } else {
            Ok(Some(rows))
        }
    }

    /// Store a meta-summary (level 2) and delete the summarized level-1 summaries
    pub async fn store_meta_summary(&self, summary: &str, summary_ids: &[String]) -> Result<()> {
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
        .bind(&self.project_path)
        .bind(summary)
        .bind(serde_json::to_string(summary_ids)?)
        .bind(summary_ids.len() as i64)
        .bind(now)
        .execute(&self.db)
        .await?;

        // Delete the old level-1 summaries
        for sum_id in summary_ids {
            sqlx::query("DELETE FROM chat_summaries WHERE id = $1")
                .bind(sum_id)
                .execute(&self.db)
                .await?;
        }

        info!(
            "Stored meta-summary, deleted {} level-1 summaries",
            summary_ids.len()
        );
        Ok(())
    }

    /// Check if summarization is needed
    /// Returns messages to summarize if threshold exceeded
    pub async fn check_summarization_needed(&self) -> Result<Option<Vec<ChatMessage>>> {
        // Count total messages
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM chat_messages")
            .fetch_one(&self.db)
            .await?;

        if count.0 as usize <= SUMMARIZE_THRESHOLD {
            return Ok(None);
        }

        // Get oldest messages outside the recent window (to be summarized)
        let to_summarize_count = count.0 as usize - RECENT_RAW_COUNT;
        if to_summarize_count < SUMMARIZE_BATCH_SIZE {
            return Ok(None);
        }

        info!(
            "Summarization needed: {} messages to compress (batch of {})",
            to_summarize_count, SUMMARIZE_BATCH_SIZE
        );

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
        .fetch_all(&self.db)
        .await?;

        use sqlx::Row;
        let messages: Vec<ChatMessage> = rows
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

                Some(ChatMessage {
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

    /// Store a summary and archive the summarized messages (no longer deletes!)
    pub async fn store_summary(&self, summary: &str, message_ids: &[String]) -> Result<()> {
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
        .bind(&self.project_path)
        .bind(summary)
        .bind(serde_json::to_string(message_ids)?)
        .bind(message_ids.len() as i64)
        .bind(now)
        .execute(&self.db)
        .await?;

        // Archive the old messages (don't delete - preserve for recall)
        for msg_id in message_ids {
            sqlx::query(
                "UPDATE chat_messages SET archived_at = $1, summary_id = $2 WHERE id = $3",
            )
            .bind(now)
            .bind(&id)
            .bind(msg_id)
            .execute(&self.db)
            .await?;
        }

        // Update message count (only active, non-archived)
        let remaining: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM chat_messages WHERE archived_at IS NULL",
        )
        .fetch_one(&self.db)
        .await?;

        sqlx::query(
            "UPDATE chat_context SET total_messages = $1, updated_at = $2 WHERE project_path = $3",
        )
        .bind(remaining.0)
        .bind(now)
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;

        info!("Stored summary, archived {} old messages", message_ids.len());
        Ok(())
    }

    /// Store a per-turn summary (doesn't delete messages - just adds summary)
    /// Used for immediate turn summarization in fresh-chain-per-turn mode
    pub async fn store_turn_summary(&self, summary: &str) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO chat_summaries (id, project_path, summary, message_ids, message_count, level, created_at)
            VALUES ($1, $2, $3, '[]', 1, 1, $4)
            "#,
        )
        .bind(&id)
        .bind(&self.project_path)
        .bind(summary)
        .bind(now)
        .execute(&self.db)
        .await?;

        debug!("Stored turn summary");
        Ok(())
    }
}
