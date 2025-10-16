// src/api/ws/chat/unified_handler.rs
// STUB: Will be completely rewritten in Phase 7 with new operation engine
// Old orchestrators (ChatOrchestrator, StreamingOrchestrator) deleted in Phase 0

use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::CompleteResponse;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
}

pub struct UnifiedChatHandler {
    _app_state: Arc<AppState>,
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { _app_state: app_state }
    }
    
    pub async fn handle_message(
        &self,
        _request: ChatRequest,
    ) -> Result<CompleteResponse> {
        Err(anyhow::anyhow!("Unified chat handler temporarily disabled - being rewritten in Phase 7"))
    }
    
    pub async fn handle_message_streaming<F>(
        &self,
        _request: ChatRequest,
        _on_event: F,
    ) -> Result<CompleteResponse>
    where
        F: FnMut(crate::llm::provider::StreamEvent) -> Result<()> + Send,
    {
        Err(anyhow::anyhow!("Streaming handler temporarily disabled - being rewritten in Phase 7"))
    }
}
