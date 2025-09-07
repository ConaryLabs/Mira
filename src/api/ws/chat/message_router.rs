// src/api/ws/chat/message_router.rs
// Routes incoming WebSocket messages to appropriate handlers based on message type.
// Manages chat messages, commands, and domain-specific operations.
// PHASE 4 UPDATE: Added full project command handling

use std::sync::Arc;
use std::net::SocketAddr;

use tracing::{debug, error, info};

use super::connection::WebSocketConnection;
use crate::api::ws::message::{WsClientMessage, WsServerMessage, MessageMetadata};
use crate::api::ws::chat_tools::handle_chat_message_with_tools;
use crate::api::ws::memory;
use crate::api::ws::project;  // Added for Phase 4
use crate::api::error::ApiError;  // Added for Phase 4
use crate::state::AppState;
use crate::config::CONFIG;

pub struct MessageRouter {
    app_state: Arc<AppState>,
    connection: Arc<WebSocketConnection>,
    addr: SocketAddr,
}

impl MessageRouter {
    pub fn new(
        app_state: Arc<AppState>,
        connection: Arc<WebSocketConnection>,
        addr: SocketAddr,
    ) -> Self {
        Self {
            app_state,
            connection,
            addr,
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
            WsClientMessage::FileTransfer { operation, data } => {
                self.handle_file_transfer(operation, data, request_id).await
            }
        }
    }

    /// Routes chat messages to tool-enabled or simple handler based on configuration
    async fn handle_chat_message(
        &self,
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
    ) -> Result<(), anyhow::Error> {
        info!("Chat message received: {} chars", content.len());
        self.connection.set_processing(true).await;

        // Use the eternal session for single-user mode
        let session_id = "peter-eternal".to_string();
        
        let result = if should_use_tools(&metadata) {
            debug!("Routing to tool-enabled handler with session_id: {}", session_id);
            
            handle_chat_message_with_tools(
                content,
                project_id,
                metadata,
                self.app_state.clone(),
                self.connection.get_sender(),
                session_id,
            ).await
        } else {
            debug!("Routing to simple chat handler");
            self.handle_simple_chat_message(content, project_id).await
        };

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

    /// Handle simple chat messages (non-tool enabled)
    async fn handle_simple_chat_message(
        &self,
        content: String,
        project_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        use super::handle_simple_chat_message;
        
        handle_simple_chat_message(
            content,
            project_id,
            self.app_state.clone(),
            self.connection.get_sender(),
            self.connection.get_last_send_ref(),
        ).await
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
        // Could implement typing indicator broadcast to other clients in multi-user mode
        Ok(())
    }

    /// PHASE 4: Handle project and artifact commands
    async fn handle_project_command(
        &self,
        method: String,
        params: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("Project command: {} with params: {:?}", method, params);
        
        // Call the actual project handler
        let result = project::handle_project_command(
            &method,
            params,
            self.app_state.clone()
        ).await;
        
        match result {
            Ok(response) => {
                // Send the successful response
                self.connection.send_message(response).await?;
                Ok(())
            }
            Err(api_error) => {
                // Convert ApiError to appropriate error response
                let error_msg = format!("{}", api_error);
                let error_code = self.api_error_to_code(&api_error);
                
                error!("Project command failed: {} - {}", error_code, error_msg);
                
                // Send error response with request_id if available
                if let Some(req_id) = request_id {
                    let error_response = WsServerMessage::Data {
                        data: serde_json::json!({
                            "error": error_msg,
                            "code": error_code
                        }),
                        request_id: Some(req_id),
                    };
                    self.connection.send_message(error_response).await?;
                } else {
                    self.connection.send_error(&error_msg, error_code.to_string()).await?;
                }
                Ok(())
            }
        }
    }

    /// Handle memory commands with request_id support
    async fn handle_memory_command(
        &self,
        method: String,
        params: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("Memory command: {} with params: {:?}", method, params);
        
        // Call the memory handler (it only takes 3 params, not 4)
        let result = memory::handle_memory_command(
            &method,
            params,
            self.app_state.clone()
        ).await;
        
        match result {
            Ok(response) => {
                self.connection.send_message(response).await?;
            }
            Err(e) => {
                let error_msg = format!("Memory operation failed: {}", e);
                
                if let Some(req_id) = request_id {
                    let response = WsServerMessage::Data {
                        data: serde_json::json!({
                            "error": error_msg
                        }),
                        request_id: Some(req_id),
                    };
                    self.connection.send_message(response).await?;
                } else {
                    self.connection.send_error(&error_msg, "MEMORY_ERROR".to_string()).await?;
                }
            }
        }
        Ok(())
    }

    /// PHASE 5: Handle git commands (stub for now)
    async fn handle_git_command(
        &self,
        method: String,
        params: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("Git command: {} with params: {:?}", method, params);
        
        // Phase 5: Will be implemented next
        let error_msg = "Git commands not yet implemented";
        
        if let Some(req_id) = request_id {
            let response = WsServerMessage::Data {
                data: serde_json::json!({
                    "error": error_msg
                }),
                request_id: Some(req_id),
            };
            self.connection.send_message(response).await?;
        } else {
            self.connection.send_error(error_msg, "NOT_IMPLEMENTED".to_string()).await?;
        }
        Ok(())
    }

    /// PHASE 6: Handle file transfer operations (stub for now)
    async fn handle_file_transfer(
        &self,
        operation: String,
        data: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("File transfer: {} with data: {:?}", operation, data);
        
        // Phase 6: Will be implemented later
        let error_msg = "File transfers not yet implemented";
        
        if let Some(req_id) = request_id {
            let response = WsServerMessage::Data {
                data: serde_json::json!({
                    "error": error_msg
                }),
                request_id: Some(req_id),
            };
            self.connection.send_message(response).await?;
        } else {
            self.connection.send_error(error_msg, "NOT_IMPLEMENTED".to_string()).await?;
        }
        Ok(())
    }

    /// Helper to convert ApiError to error code string
    fn api_error_to_code(&self, error: &ApiError) -> &'static str {
        // Match on the error message since ApiError structure isn't fully known
        let error_str = error.to_string();
        if error_str.contains("not found") {
            "NOT_FOUND"
        } else if error_str.contains("bad request") || error_str.contains("Invalid") {
            "BAD_REQUEST"
        } else if error_str.contains("unauthorized") {
            "UNAUTHORIZED"
        } else if error_str.contains("forbidden") {
            "FORBIDDEN"
        } else {
            "INTERNAL_ERROR"
        }
    }
}

/// Determines whether to use tool-enabled chat based on metadata and configuration
pub fn should_use_tools(metadata: &Option<MessageMetadata>) -> bool {
    // Check if tools are disabled globally
    if !CONFIG.enable_chat_tools {
        return false;
    }
    
    // Check metadata for context that would benefit from tools
    if let Some(meta) = metadata {
        // If we have file context, repository, or attachments, use tools
        if meta.file_path.is_some() || meta.repo_id.is_some() || meta.attachment_id.is_some() {
            debug!("Using tools due to file/repo/attachment context");
            return true;
        }
        
        // If we have language context, use tools
        if meta.language.is_some() {
            debug!("Using tools due to language context");
            return true;
        }
    }
    
    // Default to true when tools are enabled globally
    true
}

/// Extracts file context from message metadata if available
pub fn extract_file_context(metadata: &Option<MessageMetadata>) -> Option<String> {
    metadata.as_ref().and_then(|meta| {
        if let Some(file_path) = &meta.file_path {
            Some(format!("File context from: {}", file_path))
        } else if let Some(repo_id) = &meta.repo_id {
            Some(format!("Repository: {}", repo_id))
        } else if let Some(attachment_id) = &meta.attachment_id {
            Some(format!("Attachment: {}", attachment_id))
        } else {
            None
        }
    })
}
