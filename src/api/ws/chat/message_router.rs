// src/api/ws/chat/message_router.rs
// Routes incoming WebSocket messages to appropriate handlers based on message type.
// Manages chat messages, commands, and domain-specific operations.

use std::sync::Arc;
use std::net::SocketAddr;

use tracing::{debug, error, info};

use super::connection::WebSocketConnection;
use crate::api::ws::message::{WsClientMessage, MessageMetadata};
use crate::api::ws::chat_tools::handle_chat_message_with_tools;
use crate::api::ws::memory;
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

    /// Main message routing entry point
    pub async fn route_message(&self, msg: WsClientMessage) -> Result<(), anyhow::Error> {
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
                self.handle_project_command(method, params).await
            }
            WsClientMessage::MemoryCommand { method, params } => {
                self.handle_memory_command(method, params).await
            }
            WsClientMessage::GitCommand { method, params } => {
                self.handle_git_command(method, params).await
            }
            WsClientMessage::FileTransfer { operation, data } => {
                self.handle_file_transfer(operation, data).await
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
            self.handle_simple_chat_message(
                content,
                project_id,
            ).await
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

    /// Handle simple chat messages without tool support
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

    /// Handle system commands like heartbeat/ping
    async fn handle_command_message(
        &self,
        command: String,
        args: Option<serde_json::Value>,
    ) -> Result<(), anyhow::Error> {
        debug!("Command received: {} with args: {:?}", command, args);
        
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
        debug!("Typing indicator: {}", if active { "active" } else { "inactive" });
        Ok(())
    }
    
    /// Handle project management commands
    async fn handle_project_command(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        info!("Project command: {} with params: {:?}", method, params);
        self.connection.send_error("Project commands not yet implemented", "NOT_IMPLEMENTED".to_string()).await?;
        Ok(())
    }

    /// Handle memory operations (save, search, context, etc.)
    async fn handle_memory_command(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        info!("Memory command: {} with params: {:?}", method, params);
        
        match memory::handle_memory_command(&method, params, self.app_state.clone()).await {
            Ok(response) => {
                self.connection.send_message(response).await?;
                Ok(())
            }
            Err(e) => {
                error!("Memory command {} failed: {}", method, e);
                self.connection.send_error(
                    &format!("Memory operation '{}' failed: {}", method, e),
                    "MEMORY_ERROR".to_string()
                ).await?;
                Ok(())
            }
        }
    }

    /// Handle git repository operations
    async fn handle_git_command(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        info!("Git command: {} with params: {:?}", method, params);
        self.connection.send_error("Git commands not yet implemented", "NOT_IMPLEMENTED".to_string()).await?;
        Ok(())
    }

    /// Handle file transfer operations
    async fn handle_file_transfer(
        &self,
        operation: String,
        data: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        info!("File transfer: {} with data: {:?}", operation, data);
        self.connection.send_error("File transfers not yet implemented", "NOT_IMPLEMENTED".to_string()).await?;
        Ok(())
    }
}

/// Determines whether to use tool-enabled chat based on metadata and configuration
pub fn should_use_tools(metadata: &Option<MessageMetadata>) -> bool {
    // Use the enable_chat_tools field from CONFIG
    if !CONFIG.enable_chat_tools {
        return false;
    }
    
    // For now, default to true when tools are enabled globally
    // In the future, could check metadata for tool-specific flags
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
