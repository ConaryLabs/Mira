// src/memory/service/message_pipeline/coordinator.rs
use std::sync::Arc;
use anyhow::Result;
use crate::memory::{
    core::types::MemoryEntry,
    features::message_pipeline::{MessagePipeline, UnifiedAnalysis, PipelineConfig},
};

pub struct MessagePipelineCoordinator {
    pipeline: Arc<MessagePipeline>,
    config: PipelineConfig,
}

impl MessagePipelineCoordinator {
    pub fn new(pipeline: Arc<MessagePipeline>) -> Self {
        Self {
            pipeline,
            config: PipelineConfig::default(),
        }
    }

    pub async fn analyze_message(&self, entry: &MemoryEntry, role: &str) -> Result<UnifiedAnalysis> {
        // Delegate to the existing MessagePipeline
        self.pipeline.analyze_message(&entry.content, role, None).await
    }

    pub async fn process_code_element(&self, content: &str, language: &str) -> Result<UnifiedAnalysis> {
        // Future: code intelligence analysis
        // For now, treat as regular content analysis
        self.pipeline.analyze_message(content, "code", Some(language)).await
    }

    pub async fn process_pending_messages(&self, session_id: &str) -> Result<usize> {
        // Delegate to existing batch processing
        self.pipeline.process_pending_messages(session_id).await
    }
}
