// src/api/ws/chat/message_router.rs
// Routes incoming WebSocket messages to appropriate handlers.
// Uses UnifiedChatHandler for all chat messages.

use std::sync::Arc;
use std::net::SocketAddr;

use futures::StreamExt;
use tracing::{debug, error, info};

use super::connection::WebSocketConnection;
use super::unified_handler::{UnifiedChatHandler, ChatRequest, ChatEvent};
use crate::api::ws::message::{WsClientMessage, WsServerMessage, MessageMetadata};
use crate::api::ws::{memory, project, git, files, filesystem};
use crate::state::AppState;
use crate::tools::executor::ToolExecutor;

pub struct MessageRouter {
    app_state: Arc<AppState>,
    connection: Arc<WebSocketConnection>,
    addr: SocketAddr,
    unified_handler: UnifiedChatHandler,
    tool_executor: ToolExecutor,
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
            tool_executor: ToolExecutor::new(),
        }
    }

    /// Main message routing entry point with request_id support
    pub async fn route_message(&self, msg: WsClientMessage, request_id: Option<String>) -> Result<(), anyhow::Error> {
        match msg {
            WsClientMessage::Chat { content, project_id, metadata } => {
                self.handle_chat_message(content, project_id, metadata).await
            }
            WsClientMessage::Command { command, args } => {
                self.handle_command_message(command, args).await
            }
            WsClientMessage::Status { message } => {
                self.handle_status_message(message).await
            }
            WsClientMessage::Typing { active } => {
                self.handle_typing_message(active).await
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
        }
    }

    /// Routes all chat messages through the unified handler
    async fn handle_chat_message(
        &self,
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
    ) -> Result<(), anyhow::Error> {
        // Preserve exact logging from original
        info!("Chat message received: {} chars", content.len());
        
        // Preserve processing flag behavior
        self.connection.set_processing(true).await;

        // Always use "peter-eternal" session
        let session_id = "peter-eternal".to_string();
        
        // Log routing decision for debugging
        if self.tool_executor.should_use_tools(&metadata) {
            debug!("Routing to tool-enabled handler with session_id: {}", session_id);
        } else {
            debug!("Routing to simple chat handler");
        }
        
        // Create chat request
        let request = ChatRequest {
            content,
            project_id,
            metadata,
            session_id,
            require_json: false,
        };
        
        // Process through unified handler
        let result = match self.unified_handler.handle_message(request).await {
            Ok(stream) => self.stream_to_client(stream).await,
            Err(e) => Err(e),
        };

        // Clear processing flag
        self.connection.set_processing(false).await;

        if let Err(e) = result {
            error!("Error handling chat message: {}", e);
            let _ = self.connection.send_error(
                &format!("Failed to process message: {}", e),
                "PROCESSING_ERROR".to_string()
            ).await;
        }

        Ok(())
    }
    
    /// Stream chat events to the WebSocket client
    async fn stream_to_client(
        &self,
        mut stream: impl futures::Stream<Item = Result<ChatEvent, anyhow::Error>> + Unpin,
    ) -> Result<(), anyhow::Error> {
        let mut has_tools = false;
        let mut complete_sent = false;
        
        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    match event {
                        ChatEvent::Content { text } => {
                            // Stream text chunks
                            self.connection.send_message(WsServerMessage::StreamChunk { text }).await?;
                        }
                        ChatEvent::ToolExecution { tool_name, status } => {
                            // Mark that we're using tools
                            has_tools = true;
                            debug!("Tool execution: {} - {}", tool_name, status);
                            
                            // Could send status if desired
                            let detail = format!("Executing tool: {}", tool_name);
                            self.connection.send_status(&detail, Some(status)).await?;
                        }
                        ChatEvent::ToolResult { tool_name, result: _ } => {
                            debug!("Tool {} returned result", tool_name);
                            // Tool results are typically incorporated into the response
                        }
                        ChatEvent::Complete { mood, salience, tags } => {
                            // Send complete message
                            complete_sent = true;
                            self.connection.send_message(WsServerMessage::Complete {
                                mood,
                                salience,
                                tags,
                            }).await?;
                        }
                        ChatEvent::Done => {
                            // Send appropriate completion sequence
                            if !complete_sent {
                                // Send StreamEnd first
                                self.connection.send_message(WsServerMessage::StreamEnd).await?;
                                
                                // Then Complete for simple path
                                if !has_tools {
                                    self.connection.send_message(WsServerMessage::Complete {
                                        mood: Some("helpful".to_string()),
                                        salience: None,
                                        tags: None,
                                    }).await?;
                                }
                            }
                            
                            // Send Done for tool path
                            if has_tools {
                                self.connection.send_message(WsServerMessage::Done).await?;
                            }
                            
                            break;
                        }
                        ChatEvent::Error { message } => {
                            error!("Stream error: {}", message);
                            self.connection.send_error(&message, "STREAM_ERROR".to_string()).await?;
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Stream result error: {}", e);
                    self.connection.send_error(
                        &format!("Stream processing error: {}", e),
                        "STREAM_RESULT_ERROR".to_string()
                    ).await?;
                    break;
                }
            }
        }
        
        Ok(())
    }

    /// Handle command messages
    async fn handle_command_message(
        &self,
        command: String,
        args: Option<serde_json::Value>,
    ) -> Result<(), anyhow::Error> {
        info!("Command received: {} with args: {:?}", command, args);
        
        match command.as_str() {
            "ping" | "heartbeat" => {
                debug!("Heartbeat command received");
                self.connection.send_status("pong", None).await?;
            }
            _ => {
                debug!("Unknown command: {}", command);
            }
        }
        
        Ok(())
    }

    /// Handle status messages from client
    async fn handle_status_message(
        &self,
        message: String,
    ) -> Result<(), anyhow::Error> {
        debug!("Status message: {}", message);
        
        if message == "pong" || message.to_lowercase().contains("heartbeat") {
            debug!("Received heartbeat response from client");
        }
        
        Ok(())
    }

    /// Handle typing indicators
    async fn handle_typing_message(
        &self,
        active: bool,
    ) -> Result<(), anyhow::Error> {
        debug!("Typing indicator: active={}", active);
        Ok(())
    }

    /// Handle project and artifact commands
    async fn handle_project_command(
        &self,
        method: String,
        params: serde_json::Value,
        _request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("Project command: {}", method);
        
        let result = project::handle_project_command(
            &method,
            params,
            self.app_state.clone(),
        ).await;
        
        match result {
            Ok(response) => {
                self.connection.send_message(response).await?;
            }
            Err(e) => {
                error!("Project command failed: {}", e);
                self.connection.send_error(
                    &format!("Project command failed: {}", e),
                    "PROJECT_ERROR".to_string()
                ).await?;
            }
        }
        
        Ok(())
    }

    /// Handle memory commands
    async fn handle_memory_command(
        &self,
        method: String,
        params: serde_json::Value,
        _request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("Memory command: {}", method);
        
        let result = memory::handle_memory_command(
            &method,
            params,
            self.app_state.clone(),
        ).await;
        
        match result {
            Ok(response) => {
                self.connection.send_message(response).await?;
            }
            Err(e) => {
                error!("Memory command failed: {}", e);
                self.connection.send_error(
                    &format!("Memory command failed: {}", e),
                    "MEMORY_ERROR".to_string()
                ).await?;
            }
        }
        
        Ok(())
    }

    /// Handle git commands
    async fn handle_git_command(
        &self,
        method: String,
        params: serde_json::Value,
        _request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("Git command: {}", method);
        
        let result = git::handle_git_command(
            &method,
            params,
            self.app_state.clone(),
        ).await;
        
        match result {
            Ok(response) => {
                self.connection.send_message(response).await?;
            }
            Err(e) => {
                error!("Git command failed: {}", e);
                self.connection.send_error(
                    &format!("Git command failed: {}", e),
                    "GIT_ERROR".to_string()
                ).await?;
            }
        }
        
        Ok(())
    }

    /// Handle filesystem commands
    async fn handle_filesystem_command(
        &self,
        method: String,
        params: serde_json::Value,
        _request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("FileSystem command: {}", method);
        
        let result = filesystem::handle_filesystem_command(
            &method,
            params,
            self.app_state.clone(),
        ).await;
        
        match result {
            Ok(response) => {
                self.connection.send_message(response).await?;
            }
            Err(e) => {
                error!("FileSystem command failed: {}", e);
                self.connection.send_error(
                    &format!("FileSystem command failed: {}", e),
                    "FILESYSTEM_ERROR".to_string()
                ).await?;
            }
        }
        
        Ok(())
    }

    /// Handle file transfer operations
    async fn handle_file_transfer(
        &self,
        operation: String,
        data: serde_json::Value,
        _request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("File transfer: {}", operation);
        
        let result = files::handle_file_transfer(
            &operation,
            data,
            self.app_state.clone(),
        ).await;
        
        match result {
            Ok(response) => {
                self.connection.send_message(response).await?;
            }
            Err(e) => {
                error!("File transfer failed: {}", e);
                self.connection.send_error(
                    &format!("File transfer failed: {}", e),
                    "FILE_TRANSFER_ERROR".to_string()
                ).await?;
            }
        }
        
        Ok(())
    }
}
