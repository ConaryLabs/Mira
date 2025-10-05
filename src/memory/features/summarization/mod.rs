pub mod strategies;
pub mod storage;
pub mod triggers;

use std::sync::Arc;
use anyhow::Result;
use tracing::info;
use crate::llm::client::OpenAIClient;
use crate::llm::provider::LlmProvider;
use crate::memory::core::traits::MemoryStore;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::features::memory_types::SummaryType;

use strategies::{RollingSummaryStrategy, SnapshotSummaryStrategy};
use storage::SummaryStorage;
use triggers::BackgroundTriggers;

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
    /// Takes both LlmProvider (for summary generation) and OpenAIClient (for embeddings)
    pub fn new(
        llm_provider: Arc<dyn LlmProvider>,
        embedding_client: Arc<OpenAIClient>,
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
        // Check if we should create a summary
        let summary_type = self.triggers.should_create_summary(message_count);
        
        if let Some(summary_type) = summary_type {
            let window_size = match summary_type {
                SummaryType::Rolling10 => 10,
                SummaryType::Rolling100 => 100,
                SummaryType::Snapshot => return Ok(None), // Snapshots are manual only
            };
            
            // Load messages
            let messages = self.sqlite_store
                .load_recent(session_id, window_size)
                .await?;
            
            // Create summary via rolling strategy
            let summary = self.rolling_strategy
                .create_summary(session_id, &messages, window_size)
                .await?;
            
            // Store the summary
            self.storage
                .store_summary(session_id, &summary, summary_type, messages.len())
                .await?;
            
            Ok(Some(format!("Created {}-message summary", window_size)))
        } else {
            Ok(None)
        }
    }
    
    /// Manual trigger for rolling summary (API/WebSocket calls)
    pub async fn create_rolling_summary(
        &self,
        session_id: &str,
        window_size: usize,
    ) -> Result<String> {
        let summary_type = if window_size == 100 {
            SummaryType::Rolling100
        } else {
            SummaryType::Rolling10
        };
        
        let messages = self.sqlite_store
            .load_recent(session_id, window_size)
            .await?;
        
        let summary = self.rolling_strategy
            .create_summary(session_id, &messages, window_size)
            .await?;
        
        self.storage
            .store_summary(session_id, &summary, summary_type, messages.len())
            .await?;
        
        Ok(format!("Created {}-message rolling summary", window_size))
    }
    
    /// Manual trigger for snapshot summary (API/WebSocket calls)  
    pub async fn create_snapshot_summary(
        &self,
        session_id: &str,
        max_tokens: Option<usize>,
    ) -> Result<String> {
        let messages = self.sqlite_store
            .load_recent(session_id, 50) // Recent 50 for snapshot context
            .await?;
        
        let summary = self.snapshot_strategy
            .create_summary(session_id, &messages, max_tokens)
            .await?;
        
        self.storage
            .store_summary(session_id, &summary, SummaryType::Snapshot, messages.len())
            .await?;
        
        info!("Created snapshot summary for session {}", session_id);
        
        Ok(summary)
    }
    
    /// Stats for monitoring
    pub fn get_stats(&self) -> String {
        "SummarizationEngine: Rolling (10/100) + Snapshot strategies enabled".to_string()
    }
}
