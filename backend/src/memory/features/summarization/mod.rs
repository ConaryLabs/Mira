//  src/memory/features/summarization/mod.rs

pub mod storage;
pub mod strategies;
pub mod triggers;

use crate::llm_compat::provider::{EmbeddingProvider, LlmProvider};
use crate::memory::core::traits::MemoryStore;
use crate::memory::features::memory_types::SummaryType;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use storage::SummaryStorage;
use strategies::{RollingSummaryStrategy, SnapshotSummaryStrategy};
use triggers::BackgroundTriggers;

const ROLLING_WINDOW_SIZE: usize = 100;

/// Clean, focused SummarizationEngine with modular architecture
/// Delegates all operations to specialized strategy modules
pub struct SummarizationEngine {
    // Strategy modules
    rolling_strategy: RollingSummaryStrategy,
    snapshot_strategy: SnapshotSummaryStrategy,
    storage: SummaryStorage,
    triggers: BackgroundTriggers,

    // Core dependencies
    sqlite_store: Arc<SqliteMemoryStore>,
}

impl SummarizationEngine {
    /// Creates new summarization engine with all strategy modules
    pub fn new(
        llm_provider: Arc<dyn LlmProvider>,
        embedding_client: Arc<dyn EmbeddingProvider>,
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            rolling_strategy: RollingSummaryStrategy::new(llm_provider.clone()),
            snapshot_strategy: SnapshotSummaryStrategy::new(llm_provider.clone()),
            storage: SummaryStorage::new(embedding_client, sqlite_store.clone(), multi_store),
            triggers: BackgroundTriggers::new(),
            sqlite_store,
        }
    }

    /// Main entry point for background tasks - checks triggers and processes
    pub async fn check_and_process_summaries(
        &self,
        session_id: &str,
        message_count: usize,
    ) -> Result<Option<String>> {
        // Check if we should create a summary (every 100 messages)
        let summary_type = self.triggers.should_create_summary(message_count);

        if let Some(summary_type) = summary_type {
            match summary_type {
                SummaryType::Rolling => {
                    // Load messages
                    let messages = self
                        .sqlite_store
                        .load_recent(session_id, ROLLING_WINDOW_SIZE)
                        .await?;

                    // Create summary
                    let summary = self
                        .rolling_strategy
                        .create_summary(session_id, &messages)
                        .await?;

                    // Store the summary
                    self.storage
                        .store_summary(session_id, &summary, summary_type, messages.len())
                        .await?;

                    Ok(Some(summary))
                }
                SummaryType::Snapshot => Ok(None), // Snapshots are manual only
            }
        } else {
            Ok(None)
        }
    }

    /// Manual trigger for rolling summary (API/WebSocket calls)
    pub async fn create_rolling_summary(&self, session_id: &str) -> Result<String> {
        let messages = self
            .sqlite_store
            .load_recent(session_id, ROLLING_WINDOW_SIZE)
            .await?;

        let summary = self
            .rolling_strategy
            .create_summary(session_id, &messages)
            .await?;

        self.storage
            .store_summary(session_id, &summary, SummaryType::Rolling, messages.len())
            .await?;

        Ok(summary)
    }

    /// Manual trigger for snapshot summary (API/WebSocket calls)
    pub async fn create_snapshot_summary(
        &self,
        session_id: &str,
        max_tokens: Option<usize>,
    ) -> Result<String> {
        let messages = self
            .sqlite_store
            .load_recent(session_id, 50) // Recent 50 for snapshot context
            .await?;

        let summary = self
            .snapshot_strategy
            .create_summary(session_id, &messages, max_tokens)
            .await?;

        self.storage
            .store_summary(session_id, &summary, SummaryType::Snapshot, messages.len())
            .await?;

        info!("Created snapshot summary for session {}", session_id);

        Ok(summary)
    }

    /// Get the most recent rolling summary for a session
    pub async fn get_rolling_summary(&self, session_id: &str) -> Result<Option<String>> {
        let summaries = self.storage.get_latest_summaries(session_id).await?;

        let rolling_summary = summaries
            .iter()
            .find(|s| s.summary_type == "rolling")
            .map(|s| s.summary_text.clone());

        Ok(rolling_summary)
    }

    /// Get the most recent snapshot summary for a session
    pub async fn get_session_summary(&self, session_id: &str) -> Result<Option<String>> {
        let summaries = self.storage.get_latest_summaries(session_id).await?;

        let session_summary = summaries
            .iter()
            .find(|s| s.summary_type == "snapshot")
            .map(|s| s.summary_text.clone());

        Ok(session_summary)
    }

    /// Stats for monitoring
    pub fn get_stats(&self) -> String {
        "SummarizationEngine: Rolling (100-msg) + Snapshot strategies enabled".to_string()
    }
}
