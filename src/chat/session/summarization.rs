//! Message summarization
//!
//! Handles rolling summaries and meta-summarization for context compression.
//! Uses core::ops::chat_summary for all database operations.

use anyhow::Result;
use tracing::{debug, info};

use crate::core::ops::chat_summary as core_summary;
use crate::core::OpContext;

use super::{ChatMessage, SessionManager, SUMMARIZE_BATCH_SIZE, SUMMARIZE_THRESHOLD, RECENT_RAW_COUNT, META_SUMMARY_THRESHOLD};

impl SessionManager {
    /// Build OpContext for core operations
    fn summary_context(&self) -> OpContext {
        OpContext::new(std::env::current_dir().unwrap_or_default())
            .with_db(self.db.clone())
    }

    /// Load rolling summaries with tiered support
    /// Prioritizes meta-summaries (level 2) over regular summaries (level 1)
    pub(super) async fn load_summaries(&self, limit: usize) -> Result<Vec<String>> {
        let ctx = self.summary_context();
        let summaries = core_summary::load_summaries(&ctx, &self.project_path, limit).await?;
        Ok(summaries)
    }

    /// Check if meta-summarization is needed (too many level-1 summaries)
    pub async fn check_meta_summarization_needed(&self) -> Result<Option<Vec<(String, String)>>> {
        let ctx = self.summary_context();

        match core_summary::get_summaries_for_meta(&ctx, &self.project_path, META_SUMMARY_THRESHOLD).await? {
            Some(summaries) => {
                info!(
                    "Meta-summarization needed: {} level-1 summaries to compress",
                    summaries.len()
                );
                Ok(Some(
                    summaries.into_iter()
                        .map(|s| (s.id, s.summary))
                        .collect()
                ))
            }
            None => Ok(None),
        }
    }

    /// Store a meta-summary (level 2) and delete the summarized level-1 summaries
    pub async fn store_meta_summary(&self, summary: &str, summary_ids: &[String]) -> Result<()> {
        let ctx = self.summary_context();
        core_summary::store_meta_summary(&ctx, &self.project_path, summary, summary_ids).await?;

        info!(
            "Stored meta-summary, deleted {} level-1 summaries",
            summary_ids.len()
        );
        Ok(())
    }

    /// Check if summarization is needed
    /// Returns messages to summarize if threshold exceeded
    pub async fn check_summarization_needed(&self) -> Result<Option<Vec<ChatMessage>>> {
        let ctx = self.summary_context();

        match core_summary::get_messages_for_summary(
            &ctx,
            SUMMARIZE_THRESHOLD,
            RECENT_RAW_COUNT,
            SUMMARIZE_BATCH_SIZE,
        ).await? {
            Some(messages) => {
                info!(
                    "Summarization needed: {} messages to compress (batch of {})",
                    messages.len(), SUMMARIZE_BATCH_SIZE
                );
                Ok(Some(
                    messages.into_iter()
                        .map(|m| ChatMessage {
                            id: m.id,
                            role: m.role,
                            content: m.content,
                            created_at: m.created_at,
                        })
                        .collect()
                ))
            }
            None => Ok(None),
        }
    }

    /// Store a summary and archive the summarized messages (no longer deletes!)
    pub async fn store_summary(&self, summary: &str, message_ids: &[String]) -> Result<()> {
        let ctx = self.summary_context();
        let output = core_summary::store_summary(&ctx, &self.project_path, summary, message_ids).await?;

        info!("Stored summary, archived {} old messages", output.archived_count);
        Ok(())
    }

    /// Store a per-turn summary (doesn't delete messages - just adds summary)
    /// Used for immediate turn summarization in fresh-chain-per-turn mode
    pub async fn store_turn_summary(&self, summary: &str) -> Result<()> {
        let ctx = self.summary_context();
        core_summary::store_turn_summary(&ctx, &self.project_path, summary).await?;

        debug!("Stored turn summary");
        Ok(())
    }
}
