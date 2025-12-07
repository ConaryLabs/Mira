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
            SummaryType::Rolling => self.engine.create_rolling_summary(session_id).await,
            SummaryType::Snapshot => self.engine.create_snapshot_summary(session_id, None).await,
        }
    }

    pub async fn create_rolling_summary(&self, session_id: &str) -> Result<String> {
        self.engine.create_rolling_summary(session_id).await
    }

    pub async fn create_snapshot_summary(
        &self,
        session_id: &str,
        _context: Option<&str>,
    ) -> Result<String> {
        self.engine.create_snapshot_summary(session_id, None).await
    }

    /// Get the most recent rolling summary for a session
    pub async fn get_rolling_summary(&self, session_id: &str) -> Result<Option<String>> {
        self.engine.get_rolling_summary(session_id).await
    }

    /// Get the most recent snapshot summary for a session
    pub async fn get_session_summary(&self, session_id: &str) -> Result<Option<String>> {
        self.engine.get_session_summary(session_id).await
    }

    /// Check and process summaries (for background tasks)
    pub async fn check_and_process_summaries(
        &self,
        session_id: &str,
        message_count: usize,
    ) -> Result<Option<String>> {
        self.engine
            .check_and_process_summaries(session_id, message_count)
            .await
    }
}
