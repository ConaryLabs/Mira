// src/api/ws/chat/unified_handler.rs
// Updated with operation routing support

use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::api::ws::message::MessageMetadata;
use crate::api::ws::operations::OperationManager;
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
    app_state: Arc<AppState>,
    operation_manager: Arc<OperationManager>,
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        let operation_manager = Arc::new(OperationManager::new(
            app_state.operation_engine.clone(),
        ));
        
        Self {
            app_state,
            operation_manager,
        }
    }
    
    /// Route message: check if it should go to OperationEngine or regular chat
    pub async fn route_and_handle(
        &self,
        request: ChatRequest,
        ws_tx: mpsc::Sender<Value>,
    ) -> Result<()> {
        // Use LLM to determine routing
        let should_route_to_ops = self.app_state
            .message_router
            .should_route_to_operation(&request.content)
            .await
            .unwrap_or(false);
        
        if should_route_to_ops {
            // Route to OperationEngine
            let _op_id = self.operation_manager.start_operation(
                request.session_id,
                request.content,
                ws_tx,
            ).await?;
            Ok(())
        } else {
            // Route to regular chat (not implemented yet)
            Err(anyhow::anyhow!("Regular chat handler not yet implemented"))
        }
    }
    
    /// Cancel an operation
    pub async fn cancel_operation(&self, operation_id: &str) -> Result<()> {
        self.operation_manager.cancel_operation(operation_id).await
    }
    
    // Legacy methods - to be removed
    pub async fn handle_message(
        &self,
        _request: ChatRequest,
    ) -> Result<CompleteResponse> {
        Err(anyhow::anyhow!("Use route_and_handle instead"))
    }
    
    pub async fn handle_message_streaming<F>(
        &self,
        _request: ChatRequest,
        _on_event: F,
    ) -> Result<CompleteResponse>
    where
        F: FnMut(crate::llm::provider::StreamEvent) -> Result<()> + Send,
    {
        Err(anyhow::anyhow!("Use route_and_handle instead"))
    }
}
