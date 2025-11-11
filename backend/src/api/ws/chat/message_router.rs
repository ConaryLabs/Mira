// src/api/ws/chat/message_router.rs
// FIXED: Don't wrap stream/status/chat_complete in Data envelope

use std::sync::Arc;
use std::net::SocketAddr;
use anyhow::Result;
use serde_json::Value;
use tracing::{debug, error, info};
use tokio::sync::mpsc;

use super::connection::WebSocketConnection;
use super::unified_handler::{UnifiedChatHandler, ChatRequest};
use crate::api::ws::message::{WsClientMessage, WsServerMessage, MessageMetadata};
use crate::api::ws::{memory, project, git, files, filesystem, code_intelligence, documents};
use crate::state::AppState;
use crate::config::CONFIG;

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

    pub async fn route_message(&self, msg: WsClientMessage, _request_id: Option<String>) -> Result<()> {
        match msg {
            WsClientMessage::Chat { content, project_id, metadata } => {
                self.handle_chat_message(content, project_id, metadata).await
            }
            WsClientMessage::ProjectCommand { method, params } => {
                self.handle_project_command(method, params).await
            }
            WsClientMessage::MemoryCommand { method, params } => {
                self.handle_memory_command(method, params).await
            }
            WsClientMessage::GitCommand { method, params } => {
                self.handle_git_command(method, params).await
            }
            WsClientMessage::FileSystemCommand { method, params } => {
                self.handle_filesystem_command(method, params).await
            }
            WsClientMessage::FileTransfer { operation, data } => {
                self.handle_file_transfer(operation, data).await
            }
            WsClientMessage::CodeIntelligenceCommand { method, params } => {
                self.handle_code_intelligence_command(method, params).await
            }
            WsClientMessage::DocumentCommand { method, params } => {
                self.handle_document_command(method, params).await
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
        info!("Processing chat message from {} (routing via LLM)", self.addr);

        let request = ChatRequest {
            session_id: CONFIG.session_id.clone(),
            content,
            project_id,
            metadata,
        };

        // Create channel for operation events
        let (tx, mut rx) = mpsc::channel::<Value>(100);
        
        // FIXED: Spawn task to forward operation events WITHOUT double-wrapping
        let connection = self.connection.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                // Check if this is a streaming protocol message that should NOT be wrapped
                if let Some(obj) = event.as_object() {
                    if let Some(event_type) = obj.get("type").and_then(|v| v.as_str()) {
                        match event_type {
                            // These are top-level message types - send directly
                            "status" | "stream" | "chat_complete" | "stream_end" => {
                                // Convert to WsServerMessage enum variant
                                let msg = match event_type {
                                    "status" => {
                                        let status = obj.get("status")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown");
                                        WsServerMessage::Status {
                                            message: status.to_string(),
                                            detail: None,
                                        }
                                    }
                                    "stream" => {
                                        let delta = obj.get("delta")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        WsServerMessage::Stream {
                                            delta: delta.to_string(),
                                        }
                                    }
                                    "chat_complete" => {
                                        WsServerMessage::ChatComplete {
                                            user_message_id: obj.get("user_message_id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            assistant_message_id: obj.get("assistant_message_id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            content: obj.get("content")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            artifacts: obj.get("artifacts")
                                                .and_then(|v| v.as_array())
                                                .cloned()
                                                .unwrap_or_default(),
                                            thinking: obj.get("thinking")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string()),
                                        }
                                    }
                                    "stream_end" => {
                                        // Just ignore this for now, or map to something
                                        continue;
                                    }
                                    _ => unreachable!(),
                                };
                                
                                let _ = connection.send_message(msg).await;
                                continue;
                            }
                            _ => {
                                // Everything else still gets wrapped in Data
                            }
                        }
                    }
                }
                
                // Wrap in Data envelope for non-streaming messages
                let _ = connection.send_message(WsServerMessage::Data {
                    data: event,
                    request_id: None,
                }).await;
            }
        });

        // Route message through unified handler
        if let Err(e) = self.unified_handler.route_and_handle(request, tx).await {
            error!("Error routing chat message: {}", e);
            self.connection.send_message(WsServerMessage::Error {
                message: e.to_string(),
                code: "CHAT_ERROR".to_string(),
            }).await?;
        }

        Ok(())
    }

    async fn handle_project_command(
        &self,
        method: String,
        params: Value,
    ) -> Result<()> {
        let result = project::handle_project_command(
            &method,
            params,
            self.app_state.clone(),
        ).await;

        match result {
            Ok(msg) => {
                self.connection.send_message(msg).await?;
            }
            Err(e) => {
                self.connection.send_message(WsServerMessage::Error {
                    message: e.to_string(),
                    code: "PROJECT_ERROR".to_string(),
                }).await?;
            }
        }

        Ok(())
    }

    async fn handle_memory_command(
        &self,
        method: String,
        params: Value,
    ) -> Result<()> {
        let result = memory::handle_memory_command(
            &method,
            params,
            self.app_state.clone(),
        ).await;

        match result {
            Ok(msg) => {
                self.connection.send_message(msg).await?;
            }
            Err(e) => {
                self.connection.send_message(WsServerMessage::Error {
                    message: e.to_string(),
                    code: "MEMORY_ERROR".to_string(),
                }).await?;
            }
        }

        Ok(())
    }

    async fn handle_git_command(
        &self,
        method: String,
        params: Value,
    ) -> Result<()> {
        let result = git::handle_git_operation(
            &method,
            params,
            self.app_state.clone(),
        ).await;

        match result {
            Ok(msg) => {
                self.connection.send_message(msg).await?;
            }
            Err(e) => {
                self.connection.send_message(WsServerMessage::Error {
                    message: e.to_string(),
                    code: "GIT_ERROR".to_string(),
                }).await?;
            }
        }

        Ok(())
    }

    async fn handle_filesystem_command(
        &self,
        method: String,
        params: Value,
    ) -> Result<()> {
        let result = filesystem::handle_filesystem_command(
            &method,
            params,
            self.app_state.clone(),
        ).await;

        match result {
            Ok(msg) => {
                self.connection.send_message(msg).await?;
            }
            Err(e) => {
                self.connection.send_message(WsServerMessage::Error {
                    message: e.to_string(),
                    code: "FILESYSTEM_ERROR".to_string(),
                }).await?;
            }
        }

        Ok(())
    }

    async fn handle_file_transfer(
        &self,
        operation: String,
        data: Value,
    ) -> Result<()> {
        let result = files::handle_file_transfer(
            &operation,
            data,
            self.app_state.clone(),
        ).await;

        match result {
            Ok(msg) => {
                self.connection.send_message(msg).await?;
            }
            Err(e) => {
                self.connection.send_message(WsServerMessage::Error {
                    message: e.to_string(),
                    code: "FILE_ERROR".to_string(),
                }).await?;
            }
        }

        Ok(())
    }

    async fn handle_code_intelligence_command(
        &self,
        method: String,
        params: Value,
    ) -> Result<()> {
        let result = code_intelligence::handle_code_intelligence_command(
            &method,
            params,
            self.app_state.clone(),
        ).await;

        match result {
            Ok(msg) => {
                self.connection.send_message(msg).await?;
            }
            Err(e) => {
                self.connection.send_message(WsServerMessage::Error {
                    message: e.to_string(),
                    code: "CODE_INTELLIGENCE_ERROR".to_string(),
                }).await?;
            }
        }

        Ok(())
    }

    async fn handle_document_command(
        &self,
        method: String,
        params: Value,
    ) -> Result<()> {
        use documents::{DocumentHandler, DocumentCommand};
        
        let handler = DocumentHandler::new(self.app_state.clone());
        
        let command = DocumentCommand {
            method: method.clone(),
            params,
        };
        
        // Create progress channel for upload operations with progress tracking
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let connection = self.connection.clone();
        
        // Spawn task to forward progress updates to WebSocket
        tokio::spawn(async move {
            while let Some(msg) = progress_rx.recv().await {
                let _ = connection.send_message(msg).await;
            }
        });
        
        match handler.handle_command(command, Some(progress_tx)).await {
            Ok(response) => {
                self.connection.send_message(response).await?;
            }
            Err(e) => {
                error!("Document command '{}' failed: {}", method, e);
                self.connection.send_message(WsServerMessage::Error {
                    message: e.to_string(),
                    code: "DOCUMENT_ERROR".to_string(),
                }).await?;
            }
        }
        
        Ok(())
    }
}
