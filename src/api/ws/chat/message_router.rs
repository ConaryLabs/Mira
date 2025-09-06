// src/api/ws/chat/message_router.rs
// PHASE 3 UPDATE: Fixed tool detection logic for better routing
// Handles routing between simple chat and tool-enabled chat based on CONFIG

use std::sync::Arc;
use std::net::SocketAddr;

use tracing::{debug, error, info};

use super::connection::WebSocketConnection;
use crate::api::ws::message::{WsClientMessage, MessageMetadata};
use crate::api::ws::chat_tools::handle_chat_message_with_tools;
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
        }
    }

    /// Routes chat messages based on improved tool detection logic
    /// PHASE 3 FIX: Use should_use_tools() function for consistent logic
    async fn handle_chat_message(
        &self,
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
    ) -> Result<(), anyhow::Error> {
        info!("Chat message received: {} chars", content.len());
        self.connection.set_processing(true).await;

        // PHASE 3 FIX: Use the centralized tool detection logic
        let result = if should_use_tools(&metadata) {
            // Use tool-enabled streaming handler
            // Generate session ID dynamically for each WebSocket session
            let session_id = format!("ws-{}-{}", 
                chrono::Utc::now().timestamp(), 
                self.addr.port()
            );
            
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
            // Use simple streaming handler
            debug!("Routing to simple chat handler");
            self.handle_simple_chat_message(
                content,
                project_id,
            ).await
        };

        self.connection.set_processing(false).await;

        if let Err(e) = result {
            error!("Error handling chat message: {}", e);
            // FIXED: Added the missing error code parameter
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
        // Call the simple chat handler from the main chat module
        use super::handle_simple_chat_message;
        
        // FIXED: Corrected parameter order to match the function signature in mod.rs
        // Function takes 5 parameters: content, project_id, app_state, sender, last_send_ref
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
        
        // Handle specific commands
        match command.as_str() {
            "ping" | "heartbeat" => {
                debug!("Heartbeat command received");
                // FIXED: Added the missing detail parameter
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
            debug!("Heartbeat acknowledged");
        }
        
        Ok(())
    }

    /// Handle typing indicator messages
    async fn handle_typing_message(
        &self,
        active: bool,
    ) -> Result<(), anyhow::Error> {
        debug!("Typing indicator: active={}", active);
        // Could implement typing indicator broadcast to other clients here
        Ok(())
    }
}

/// Determine if we should use tools based on configuration and metadata
/// PHASE 3 FIX: Centralized logic for consistent tool routing decisions
pub fn should_use_tools(metadata: &Option<MessageMetadata>) -> bool {
    // First check if tools are globally enabled in config
    if !CONFIG.enable_chat_tools {
        debug!("Tools globally disabled in config");
        return false;
    }

    // Check if client has metadata indicating it's in a context where tools would be helpful
    // FIXED: Use the actual MessageMetadata fields instead of non-existent ones
    if let Some(meta) = metadata {
        // If we have file context (file_path, repo_id, attachment_id), use tools
        if meta.file_path.is_some() || meta.repo_id.is_some() || meta.attachment_id.is_some() {
            debug!("Using tools due to file/repo/attachment context");
            return true;
        }
        
        // If we have language context, use tools
        if meta.language.is_some() {
            debug!("Using tools due to language context");
            return true;
        }
    } else {
        // No metadata = likely a basic client, don't use tools
        debug!("No metadata provided, not using tools");
        return false;
    }

    // Metadata exists but no specific context - use tools if enabled (tool-capable client)
    debug!("Using default tool behavior for metadata-capable client: {}", CONFIG.enable_chat_tools);
    CONFIG.enable_chat_tools
}

/// Extract context from uploaded files (placeholder implementation)
/// PHASE 3: This should be implemented to process file uploads
/// FIXED: Use actual MessageMetadata fields
pub fn extract_file_context(metadata: &Option<MessageMetadata>) -> Option<String> {
    metadata.as_ref().and_then(|meta| {
        if let Some(file_path) = &meta.file_path {
            Some(format!("File: {}", file_path))
        } else if let Some(repo_id) = &meta.repo_id {
            Some(format!("Repository: {}", repo_id))
        } else if let Some(attachment_id) = &meta.attachment_id {
            Some(format!("Attachment: {}", attachment_id))
        } else {
            None
        }
    })
}
