// src/api/ws/chat/message_router.rs

use std::sync::Arc;
use std::net::SocketAddr;
use anyhow::Result;
use serde_json::{json, Value};
use tracing::{debug, error, info};

use super::connection::WebSocketConnection;
use super::unified_handler::{UnifiedChatHandler, ChatRequest};
use crate::api::ws::message::{WsClientMessage, WsServerMessage, MessageMetadata};
use crate::api::ws::{memory, project, git, files, filesystem, code_intelligence};
use crate::llm::structured::CompleteResponse;
use crate::state::AppState;

pub struct MessageRouter {
    app_state: Arc<AppState>,
    connection: Arc<WebSocketConnection>,
    addr: SocketAddr,
    unified_handler: UnifiedChatHandler,
}

impl MessageRouter {
    pub fn new(
        app_state: Arc<AppState>,
        connection: Arc<WebSocketConnection>,
        addr: SocketAddr,
    ) -> Self {
        let unified_handler = UnifiedChatHandler::new(app_state.clone());
        
        Self {
            app_state,
            connection,
            addr,
            unified_handler,
        }
    }

    pub async fn route_message(&self, msg: WsClientMessage, request_id: Option<String>) -> Result<()> {
        match msg {
            WsClientMessage::Chat { content, project_id, metadata } => {
                self.handle_chat_message(content, project_id, metadata).await
            }
            WsClientMessage::ProjectCommand { method, params } => {
                self.handle_project_command(method, params, request_id).await
            }
            WsClientMessage::MemoryCommand { method, params } => {
                self.handle_memory_command(method, params, request_id).await
            }
            WsClientMessage::GitCommand { method, params } => {
                self.handle_git_command(method, params, request_id).await
            }
            WsClientMessage::FileSystemCommand { method, params } => {
                self.handle_filesystem_command(method, params, request_id).await
            }
            WsClientMessage::FileTransfer { operation, data } => {
                self.handle_file_transfer(operation, data, request_id).await
            }
            WsClientMessage::CodeIntelligenceCommand { method, params } => {
                self.handle_code_intelligence_command(method, params, request_id).await
            }
            _ => {
                debug!("Ignoring message type");
                Ok(())
            }
        }
    }

    async fn handle_chat_message(
        &self,
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
    ) -> Result<()> {
        info!("Chat message received: {} chars", content.len());
        
        self.connection.set_processing(true).await;

        let session_id = "peter-eternal".to_string();
        
        let request = ChatRequest {
            content,
            project_id,
            metadata,
            session_id,
        };
        
        let result = match self.unified_handler.handle_message(request).await {
            Ok(complete_response) => {
                self.send_complete_response_to_client(complete_response).await
            }
            Err(e) => {
                error!("Error processing chat: {}", e);
                self.connection.send_error(
                    &format!("Failed to process message: {}", e),
                    "PROCESSING_ERROR".to_string()
                ).await
            }
        };

        self.connection.set_processing(false).await;
        result
    }
    
    async fn send_complete_response_to_client(
        &self,
        complete_response: CompleteResponse,
    ) -> Result<()> {
        let response_data = json!({
            "content": complete_response.structured.output,
            "analysis": {
                "salience": complete_response.structured.analysis.salience,
                "topics": complete_response.structured.analysis.topics,
                "contains_code": complete_response.structured.analysis.contains_code,
                "routed_to_heads": complete_response.structured.analysis.routed_to_heads,
                "language": complete_response.structured.analysis.language,
                // Optional fields (can be null)
                "mood": complete_response.structured.analysis.mood,
                "intensity": complete_response.structured.analysis.intensity,
                "intent": complete_response.structured.analysis.intent,
                "summary": complete_response.structured.analysis.summary,
                "relationship_impact": complete_response.structured.analysis.relationship_impact,
                "programming_lang": complete_response.structured.analysis.programming_lang
            },
            "metadata": {
                "response_id": complete_response.metadata.response_id,
                "total_tokens": complete_response.metadata.total_tokens,
                "latency_ms": complete_response.metadata.latency_ms
            }
        });

        self.connection.send_message(WsServerMessage::Response {
            data: response_data,
        }).await?;

        Ok(())
    }

    async fn handle_project_command(&self, method: String, params: Value, request_id: Option<String>) -> Result<()> {
        let response = project::handle_project_command(&method, params, self.app_state.clone()).await?;
        
        match response {
            WsServerMessage::Data { data, .. } => {
                self.connection.send_message(WsServerMessage::Data { data, request_id }).await?;
            }
            WsServerMessage::Status { message, detail } => {
                self.connection.send_message(WsServerMessage::Status { message, detail }).await?;
            }
            _ => {
                let data = self.extract_response_data(response);
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
        }
        
        Ok(())
    }

    async fn handle_memory_command(&self, method: String, params: Value, request_id: Option<String>) -> Result<()> {
        let response = memory::handle_memory_command(&method, params, self.app_state.clone()).await?;
        
        match response {
            WsServerMessage::Data { data, .. } => {
                self.connection.send_message(WsServerMessage::Data { data, request_id }).await?;
            }
            _ => {
                let data = self.extract_response_data(response);
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
        }
        
        Ok(())
    }

    async fn handle_git_command(&self, method: String, params: Value, request_id: Option<String>) -> Result<()> {
        let response = git::handle_git_command(&method, params, self.app_state.clone()).await?;
        
        match response {
            WsServerMessage::Data { data, .. } => {
                self.connection.send_message(WsServerMessage::Data { data, request_id }).await?;
            }
            WsServerMessage::Status { message, detail } => {
                self.connection.send_message(WsServerMessage::Status { message, detail }).await?;
            }
            _ => {
                let data = self.extract_response_data(response);
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
        }
        
        Ok(())
    }

    async fn handle_filesystem_command(&self, method: String, params: Value, request_id: Option<String>) -> Result<()> {
        let response = filesystem::handle_filesystem_command(&method, params, self.app_state.clone()).await?;
        
        match response {
            WsServerMessage::Data { data, .. } => {
                self.connection.send_message(WsServerMessage::Data { data, request_id }).await?;
            }
            WsServerMessage::Status { message, detail } => {
                self.connection.send_message(WsServerMessage::Status { message, detail }).await?;
            }
            _ => {
                let data = self.extract_response_data(response);
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
        }
        
        Ok(())
    }

    async fn handle_file_transfer(&self, operation: String, data: Value, request_id: Option<String>) -> Result<()> {
        let response = files::handle_file_transfer(&operation, data, self.app_state.clone()).await?;
        
        match response {
            WsServerMessage::Data { data, .. } => {
                self.connection.send_message(WsServerMessage::Data { data, request_id }).await?;
            }
            WsServerMessage::Status { message, detail } => {
                self.connection.send_message(WsServerMessage::Status { message, detail }).await?;
            }
            _ => {
                let response_data = self.extract_response_data(response);
                self.connection.send_message(WsServerMessage::Response { data: response_data }).await?;
            }
        }
        
        Ok(())
    }

    async fn handle_code_intelligence_command(&self, method: String, params: Value, request_id: Option<String>) -> Result<()> {
        let response = code_intelligence::handle_code_intelligence_command(&method, params, self.app_state.clone()).await?;
        
        match response {
            WsServerMessage::Data { data, .. } => {
                self.connection.send_message(WsServerMessage::Data { data, request_id }).await?;
            }
            WsServerMessage::Status { message, detail } => {
                self.connection.send_message(WsServerMessage::Status { message, detail }).await?;
            }
            _ => {
                let data = self.extract_response_data(response);
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
        }
        
        Ok(())
    }
    
    fn extract_response_data(&self, response: WsServerMessage) -> Value {
        match response {
            WsServerMessage::Response { data } => data,
            WsServerMessage::Error { message, .. } => json!({"error": message}),
            _ => json!({"status": "success"}),
        }
    }
}
