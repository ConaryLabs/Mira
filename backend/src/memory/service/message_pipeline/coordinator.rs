// src/memory/service/message_pipeline/coordinator.rs

use anyhow::Result;
use std::sync::Arc;

use crate::memory::{
    core::types::MemoryEntry,
    features::message_pipeline::{MessagePipeline, UnifiedAnalysisResult},
};

pub struct MessagePipelineCoordinator {
    pipeline: Arc<MessagePipeline>,
}

impl MessagePipelineCoordinator {
    pub fn new(pipeline: Arc<MessagePipeline>) -> Self {
        Self { pipeline }
    }

    /// Get reference to underlying pipeline for direct access
    pub fn get_pipeline(&self) -> &Arc<MessagePipeline> {
        &self.pipeline
    }

    pub async fn analyze_message(
        &self,
        entry: &MemoryEntry,
        role: &str,
    ) -> Result<UnifiedAnalysisResult> {
        // Use the coordinator-compatible method that returns UnifiedAnalysisResult
        self.pipeline
            .analyze_message_for_coordinator(&entry.content, role, None)
            .await
    }

    pub async fn process_code_element(
        &self,
        content: &str,
        language: &str,
    ) -> Result<UnifiedAnalysisResult> {
        // Future: code intelligence analysis
        // For now, treat as regular content analysis
        self.pipeline
            .analyze_message_for_coordinator(content, "code", Some(language))
            .await
    }

    pub async fn process_pending_messages(&self, session_id: &str) -> Result<usize> {
        // Delegate to existing batch processing
        self.pipeline.process_pending_messages(session_id).await
    }
}
