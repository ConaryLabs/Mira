// src/api/ws/chat/message_router.rs
// REFACTORED: Extracted common command handling pattern to reduce duplication

use anyhow::Result;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use super::connection::WebSocketConnection;
use super::unified_handler::{ChatRequest, UnifiedChatHandler};
use crate::api::error::ApiError;
use crate::api::ws::message::{MessageMetadata, WsClientMessage, WsServerMessage};
use crate::api::ws::{code_intelligence, documents, files, filesystem, git, memory, project, terminal};
use crate::config::CONFIG;
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

    pub async fn route_message(&self, msg: WsClientMessage) -> Result<()> {
        match msg {
            WsClientMessage::Chat {
                content,
                project_id,
                metadata,
            } => {
                self.handle_chat_message(content, project_id, metadata)
                    .await
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
            WsClientMessage::TerminalCommand { method, params } => {
                self.handle_terminal_command(method, params).await
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
        info!(
            "Processing chat message from {} (routing via LLM)",
            self.addr
        );

        let request = ChatRequest {
            session_id: CONFIG.session_id.clone(),
            content,
            project_id,
            metadata,
        };

        // Create channel for operation events
        let (tx, mut rx) = mpsc::channel::<Value>(100);

        // Spawn task to forward operation events
        let connection = self.connection.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                // Check if this is a streaming protocol message
                let msg = Self::convert_event_to_message(event);
                let _ = connection.send_message(msg).await;
            }
        });

        // Route message through unified handler
        if let Err(e) = self.unified_handler.route_and_handle(request, tx).await {
            error!("Error routing chat message: {}", e);
            self.connection
                .send_message(WsServerMessage::Error {
                    message: e.to_string(),
                    code: "CHAT_ERROR".to_string(),
                })
                .await?;
        }

        Ok(())
    }

    /// Convert event JSON to appropriate WsServerMessage
    fn convert_event_to_message(event: Value) -> WsServerMessage {
        if let Some(obj) = event.as_object()
            && let Some(event_type) = obj.get("type").and_then(|v| v.as_str())
        {
            match event_type {
                "status" => {
                    return WsServerMessage::Status {
                        message: obj
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        detail: None,
                    };
                }
                "stream" => {
                    return WsServerMessage::Stream {
                        delta: obj
                            .get("delta")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    };
                }
                "chat_complete" => {
                    return WsServerMessage::ChatComplete {
                        user_message_id: obj
                            .get("user_message_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        assistant_message_id: obj
                            .get("assistant_message_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        content: obj
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        artifacts: obj
                            .get("artifacts")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default(),
                        thinking: obj
                            .get("thinking")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    };
                }
                "stream_end" => {
                    // Ignore stream_end events
                    return WsServerMessage::Status {
                        message: "stream_end".to_string(),
                        detail: None,
                    };
                }
                _ => {}
            }
        }

        // Wrap in Data envelope for non-streaming messages
        WsServerMessage::Data {
            data: event,
            request_id: None,
        }
    }

    /// Helper to send response or error
    async fn send_result(
        &self,
        result: Result<WsServerMessage, ApiError>,
        error_code: &str,
    ) -> Result<()> {
        match result {
            Ok(msg) => self.connection.send_message(msg).await?,
            Err(e) => {
                self.connection
                    .send_message(WsServerMessage::Error {
                        message: e.to_string(),
                        code: format!("{}_ERROR", error_code),
                    })
                    .await?
            }
        }
        Ok(())
    }

    /// Git command handler (uses anyhow::Error)
    async fn handle_git_command(&self, method: String, params: Value) -> Result<()> {
        let result = git::handle_git_operation(&method, params, self.app_state.clone()).await;
        match result {
            Ok(msg) => self.connection.send_message(msg).await?,
            Err(e) => {
                error!("Git command '{}' failed: {}", method, e);
                self.connection
                    .send_message(WsServerMessage::Error {
                        message: e.to_string(),
                        code: "GIT_ERROR".to_string(),
                    })
                    .await?
            }
        }
        Ok(())
    }

    async fn handle_project_command(&self, method: String, params: Value) -> Result<()> {
        let result = project::handle_project_command(&method, params, self.app_state.clone()).await;
        self.send_result(result, "PROJECT").await
    }

    async fn handle_memory_command(&self, method: String, params: Value) -> Result<()> {
        let result = memory::handle_memory_command(&method, params, self.app_state.clone()).await;
        self.send_result(result, "MEMORY").await
    }

    async fn handle_filesystem_command(&self, method: String, params: Value) -> Result<()> {
        let result =
            filesystem::handle_filesystem_command(&method, params, self.app_state.clone()).await;
        self.send_result(result, "FILESYSTEM").await
    }

    async fn handle_file_transfer(&self, operation: String, data: Value) -> Result<()> {
        let result = files::handle_file_transfer(&operation, data, self.app_state.clone()).await;
        self.send_result(result, "FILE").await
    }

    async fn handle_code_intelligence_command(&self, method: String, params: Value) -> Result<()> {
        let result = code_intelligence::handle_code_intelligence_command(
            &method,
            params,
            self.app_state.clone(),
        )
        .await;
        self.send_result(result, "CODE_INTELLIGENCE").await
    }

    /// Document commands need special handling for progress tracking
    async fn handle_document_command(&self, method: String, params: Value) -> Result<()> {
        use documents::{DocumentCommand, DocumentHandler};

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
                self.connection
                    .send_message(WsServerMessage::Error {
                        message: e.to_string(),
                        code: "DOCUMENT_ERROR".to_string(),
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Terminal command handler
    async fn handle_terminal_command(&self, method: String, params: Value) -> Result<()> {
        let manager = self.app_state.terminal_session_manager.clone();
        let app_state = self.app_state.clone();

        let result = match method.as_str() {
            "start_session" => {
                // Create channel for terminal output
                let (output_tx, mut output_rx) = mpsc::unbounded_channel();
                let connection = self.connection.clone();

                // Spawn task to forward terminal output to WebSocket
                tokio::spawn(async move {
                    while let Some(msg) = output_rx.recv().await {
                        let _ = connection.send_message(msg).await;
                    }
                });

                terminal::handle_start_session(params, manager, app_state, Some(output_tx)).await
            }
            "send_input" => terminal::handle_send_input(params, manager).await,
            "resize" => terminal::handle_resize(params, manager).await,
            "close_session" => terminal::handle_close_session(params, manager, app_state).await,
            "list_sessions" => terminal::handle_list_sessions(params, manager, app_state).await,
            _ => Err(ApiError::bad_request(format!("Unknown terminal method: {}", method))),
        };

        self.send_result(result, "TERMINAL").await
    }
}
