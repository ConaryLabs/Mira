// src/api/ws/chat/message_router.rs

use std::sync::Arc;
use std::net::SocketAddr;
use anyhow::Result;
use serde_json::{json, Value};
use tracing::{debug, error, info};
use tokio::sync::mpsc;

use super::connection::WebSocketConnection;
use super::unified_handler::{UnifiedChatHandler, ChatRequest};
use crate::api::ws::message::{WsClientMessage, WsServerMessage, MessageMetadata};
use crate::api::ws::{memory, project, git, files, filesystem, code_intelligence, documents};
use crate::state::AppState;
use crate::config::CONFIG; // NEW: use configured session id

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
            WsClientMessage::DocumentCommand { method, params } => {
                self.handle_document_command(method, params, request_id).await
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
        info!("Processing chat message from {}", self.addr);

        // Use configured session id instead of hardcoded one
        let request = ChatRequest {
            session_id: CONFIG.session_id.clone(),
            content,
            project_id,
            metadata,
        };

        match self.unified_handler.handle_message(request).await {
            Ok(complete_response) => {
                self.send_complete_response_to_client(complete_response).await?;
            }
            Err(e) => {
                error!("Error handling chat message: {}", e);
                self.connection.send_message(WsServerMessage::Error {
                    message: e.to_string(),
                    code: "CHAT_ERROR".to_string(),
                }).await?;
            }
        }

        Ok(())
    }

    async fn send_complete_response_to_client(
        &self,
        complete_response: crate::llm::structured::CompleteResponse,
    ) -> Result<()> {
        // Emit artifact-created events explicitly so the frontend can pop the viewer
        if let Some(ref artifacts) = complete_response.artifacts {
            for artifact in artifacts {
                self.connection
                    .send_message(WsServerMessage::Data {
                        data: json!({
                            "type": "artifact_created",
                            "artifact": artifact,
                        }),
                        request_id: None,
                    })
                    .await?;
            }
        }

        let response_data = json!({
            "content": complete_response.structured.output,
            "analysis": {
                "salience": complete_response.structured.analysis.salience,
                "topics": complete_response.structured.analysis.topics,
                "contains_code": complete_response.structured.analysis.contains_code,
                "routed_to_heads": complete_response.structured.analysis.routed_to_heads,
                "language": complete_response.structured.analysis.language,
                // Optional fields
                "mood": complete_response.structured.analysis.mood,
                "intensity": complete_response.structured.analysis.intensity,
                "intent": complete_response.structured.analysis.intent,
                "summary": complete_response.structured.analysis.summary,
                "relationship_impact": complete_response.structured.analysis.relationship_impact,
                "programming_lang": complete_response.structured.analysis.programming_lang,
            },
            "metadata": {
                "response_id": complete_response.metadata.response_id,
                "total_tokens": complete_response.metadata.total_tokens,
                "latency_ms": complete_response.metadata.latency_ms,
            },
            "artifacts": complete_response.artifacts,
        });

        self.connection.send_message(WsServerMessage::Response { 
            data: response_data 
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
            WsServerMessage::Error { message, code } => {
                self.connection.send_message(WsServerMessage::Error { message, code }).await?;
            }
            WsServerMessage::Response { data } => {
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
            _ => {
                self.connection.send_message(response).await?;
            }
        }
        
        Ok(())
    }

    async fn handle_memory_command(&self, method: String, params: Value, request_id: Option<String>) -> Result<()> {
        let response = memory::handle_memory_command(&method, params, self.app_state.clone()).await?;
        
        match response {
            WsServerMessage::Data { data, .. } => {
                let bytes = data.to_string().len();
                info!(bytes, "Sending memory data");
                self.connection.send_message(WsServerMessage::Data { data, request_id }).await?;
            }
            WsServerMessage::Status { message, detail } => {
                self.connection.send_message(WsServerMessage::Status { message, detail }).await?;
            }
            WsServerMessage::Error { message, code } => {
                self.connection.send_message(WsServerMessage::Error { message, code }).await?;
            }
            WsServerMessage::Response { data } => {
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
            _ => {
                self.connection.send_message(response).await?;
            }
        }
        
        Ok(())
    }

    async fn handle_git_command(&self, method: String, params: Value, request_id: Option<String>) -> Result<()> {
        let response = git::handle_git_operation(&method, params, self.app_state.clone()).await?;
        
        match response {
            WsServerMessage::Data { data, .. } => {
                self.connection.send_message(WsServerMessage::Data { data, request_id }).await?;
            }
            WsServerMessage::Status { message, detail } => {
                self.connection.send_message(WsServerMessage::Status { message, detail }).await?;
            }
            WsServerMessage::Error { message, code } => {
                self.connection.send_message(WsServerMessage::Error { message, code }).await?;
            }
            WsServerMessage::Response { data } => {
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
            _ => {
                self.connection.send_message(response).await?;
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
            WsServerMessage::Error { message, code } => {
                self.connection.send_message(WsServerMessage::Error { message, code }).await?;
            }
            WsServerMessage::Response { data } => {
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
            _ => {
                self.connection.send_message(response).await?;
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
            WsServerMessage::Error { message, code } => {
                self.connection.send_message(WsServerMessage::Error { message, code }).await?;
            }
            WsServerMessage::Response { data } => {
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
            _ => {
                self.connection.send_message(response).await?;
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
            WsServerMessage::Error { message, code } => {
                self.connection.send_message(WsServerMessage::Error { message, code }).await?;
            }
            WsServerMessage::Response { data } => {
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
            _ => {
                self.connection.send_message(response).await?;
            }
        }
        
        Ok(())
    }

    async fn handle_document_command(&self, method: String, params: Value, request_id: Option<String>) -> Result<()> {
        // Create a channel for progress updates
        let (tx, mut rx) = mpsc::unbounded_channel();
        
        // Spawn a task to forward progress updates
        let connection = self.connection.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let _ = connection.send_message(msg).await;
            }
        });
        
        // Create document handler with AppState
        let handler = documents::DocumentHandler::new(self.app_state.clone());
        
        let command = documents::DocumentCommand { method, params };
        let response = handler.handle_command(command, Some(tx)).await?;
        
        // Forward the response
        match response {
            WsServerMessage::Data { data, .. } => {
                self.connection.send_message(WsServerMessage::Data { data, request_id }).await?;
            }
            WsServerMessage::Status { message, detail } => {
                self.connection.send_message(WsServerMessage::Status { message, detail }).await?;
            }
            WsServerMessage::Error { message, code } => {
                self.connection.send_message(WsServerMessage::Error { message, code }).await?;
            }
            WsServerMessage::Response { data } => {
                self.connection.send_message(WsServerMessage::Response { data }).await?;
            }
            _ => {
                self.connection.send_message(response).await?;
            }
        }
        
        Ok(())
    }
}
