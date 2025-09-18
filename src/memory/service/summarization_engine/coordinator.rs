// src/memory/service/summarization_engine/coordinator.rs
use std::sync::Arc;
use anyhow::Result;
use crate::memory::features::{
    summarization::SummarizationEngine,
    memory_types::SummaryType,
};

pub struct SummarizationEngineCoordinator {
    engine: Arc<SummarizationEngine>,
}

impl SummarizationEngineCoordinator {
    pub fn new(engine: Arc<SummarizationEngine>) -> Self {
        Self { engine }
    }

    pub async fn create_summary(&self, session_id: &str, summary_type: SummaryType) -> Result<String> {
        match summary_type {
            SummaryType::Rolling10 => self.engine.create_rolling_summary(session_id, 10).await,
            SummaryType::Rolling100 => self.engine.create_rolling_summary(session_id, 100).await,
            SummaryType::Snapshot => self.engine.create_snapshot_summary(session_id, None).await,
        }
    }

    pub async fn create_rolling_summary(&self, session_id: &str, window_size: usize) -> Result<String> {
        self.engine.create_rolling_summary(session_id, window_size).await
    }

    pub async fn create_snapshot_summary(&self, session_id: &str, _context: Option<&str>) -> Result<String> {
        // SummarizationEngine expects Option<usize> for max_tokens, not Option<&str>
        // For now, just pass None for max_tokens
        self.engine.create_snapshot_summary(session_id, None).await
    }

    /// Missing method: check_and_process_summaries (for background tasks)
    pub async fn check_and_process_summaries(&self, session_id: &str, message_count: usize) -> Result<Option<String>> {
        // This method was being called by background tasks
        // For now, just trigger a rolling summary if needed
        if message_count > 0 && message_count % 10 == 0 {
            let summary = self.create_rolling_summary(session_id, 10).await?;
            Ok(Some(summary))
        } else {
            Ok(None)
        }
    }
}
