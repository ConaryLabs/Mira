// src/api/ws/chat/unified_handler.rs
// GPT 5.1 chat handler - routes ALL messages to OperationEngine

use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::api::ws::message::MessageMetadata;
use crate::api::ws::operations::OperationManager;
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
        let operation_manager = Arc::new(OperationManager::new(app_state.operation_engine.clone()));

        Self {
            app_state,
            operation_manager,
        }
    }

    /// Route all messages to OperationEngine (GPT 5.1 architecture)
    pub async fn route_and_handle(
        &self,
        request: ChatRequest,
        ws_tx: mpsc::Sender<Value>,
    ) -> Result<()> {
        info!(
            "[GPT5] Routing to OperationEngine: {}",
            request.content.chars().take(50).collect::<String>()
        );

        let _op_id = self
            .operation_manager
            .start_operation(request.session_id, request.content, ws_tx)
            .await?;

        Ok(())
    }


    /// Cancel an operation
    pub async fn cancel_operation(&self, operation_id: &str) -> Result<()> {
        self.operation_manager.cancel_operation(operation_id).await
    }
}
