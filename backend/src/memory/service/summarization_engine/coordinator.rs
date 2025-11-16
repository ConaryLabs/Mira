// src/memory/service/summarization_engine/coordinator.rs
use crate::memory::features::{memory_types::SummaryType, summarization::SummarizationEngine};
use anyhow::Result;
use std::sync::Arc;

pub struct SummarizationEngineCoordinator {
    engine: Arc<SummarizationEngine>,
}

impl SummarizationEngineCoordinator {
    pub fn new(engine: Arc<SummarizationEngine>) -> Self {
        Self { engine }
    }

    pub async fn create_summary(
        &self,
        session_id: &str,
        summary_type: SummaryType,
    ) -> Result<String> {
        match summary_type {
            SummaryType::Rolling10 => self.engine.create_rolling_summary(session_id, 10).await,
            SummaryType::Rolling100 => self.engine.create_rolling_summary(session_id, 100).await,
            SummaryType::Snapshot => self.engine.create_snapshot_summary(session_id, None).await,
        }
    }

    pub async fn create_rolling_summary(
        &self,
        session_id: &str,
        window_size: usize,
    ) -> Result<String> {
        self.engine
            .create_rolling_summary(session_id, window_size)
            .await
    }

    pub async fn create_snapshot_summary(
        &self,
        session_id: &str,
        _context: Option<&str>,
    ) -> Result<String> {
        // SummarizationEngine expects Option<usize> for max_tokens, not Option<&str>
        // For now, just pass None for max_tokens
        self.engine.create_snapshot_summary(session_id, None).await
    }

    /// Get the most recent rolling summary (100-message) for a session
    pub async fn get_rolling_summary(&self, session_id: &str) -> Result<Option<String>> {
        // Delegate to engine's public method
        self.engine.get_rolling_summary(session_id).await
    }

    /// Get the most recent snapshot summary for a session
    pub async fn get_session_summary(&self, session_id: &str) -> Result<Option<String>> {
        // Delegate to engine's public method
        self.engine.get_session_summary(session_id).await
    }

    /// Check and process summaries (for background tasks)
    /// FIXED: Now delegates to engine which has proper rolling_10/rolling_100 trigger logic
    pub async fn check_and_process_summaries(
        &self,
        session_id: &str,
        message_count: usize,
    ) -> Result<Option<String>> {
        // Delegate to engine's method which properly checks triggers
        // and creates both rolling_10 (every 10) and rolling_100 (every 100) as needed
        self.engine
            .check_and_process_summaries(session_id, message_count)
            .await
    }
}
