// src/api/ws/chat/message_router.rs
// Phase 2: Extract Message Routing from chat.rs
// Handles routing between simple chat and tool-enabled chat based on CONFIG.enable_chat_tools
// Maintains CRITICAL integration with chat_tools.rs

use std::sync::Arc;
use std::net::SocketAddr;

use tracing::{debug, error, info};

// FIXED: Updated import path - now in same directory
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

    /// CRITICAL: Routes chat messages based on CONFIG.enable_chat_tools and metadata presence
    /// This preserves the exact logic from the original chat.rs file
    async fn handle_chat_message(
        &self,
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
    ) -> Result<(), anyhow::Error> {
        info!("💬 Chat message received: {} chars", content.len());
        self.connection.set_processing(true).await;

        // CRITICAL: Check if tools are enabled (from CONFIG) - matches original logic
        let enable_tools = CONFIG.enable_chat_tools;

        // Route to appropriate handler based on tools setting and metadata presence
        let result = if enable_tools && metadata.is_some() {
            // Use tool-enabled streaming handler - CRITICAL integration with chat_tools.rs
            let session_id = CONFIG.session_id.clone();
            
            handle_chat_message_with_tools(
                content,
                project_id,
                metadata,
                self.app_state.clone(),
                self.connection.get_sender(), // Get compatible sender reference
                session_id,
            ).await
        } else {
            // Use simple streaming handler - delegate to original function
            self.handle_simple_chat_message(
                content,
                project_id,
            ).await
        };

        self.connection.set_processing(false).await;

        if let Err(e) = result {
            error!("❌ Error handling chat message: {}", e);
            let _ = self.connection.send_error(&format!("Failed to process message: {}", e)).await;
        }

        Ok(())
    }

    /// Handle simple chat messages (non-tool enabled)
    /// This extracts the simple chat logic from the original handle_chat_message function
    async fn handle_simple_chat_message(
        &self,
        content: String,
        project_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        // Call the extracted simple chat handler from the main chat.rs
        use super::handle_simple_chat_message;
        
        handle_simple_chat_message(
            content,
            project_id,
            self.app_state.clone(),
            self.connection.get_sender(),
            self.addr,
            self.connection.get_last_send_ref(),
        ).await
    }

    /// Handle command messages
    async fn handle_command_message(
        &self,
        command: String,
        args: Option<serde_json::Value>,
    ) -> Result<(), anyhow::Error> {
        info!("🎮 Command received: {} with args: {:?}", command, args);
        
        // Handle specific commands
        match command.as_str() {
            "ping" | "heartbeat" => {
                debug!("💓 Heartbeat command received");
                self.connection.send_status("pong").await?;
            }
            _ => {
                debug!("🎮 Unknown command: {}", command);
            }
        }
        
        Ok(())
    }

    /// Handle status messages from client
    async fn handle_status_message(
        &self,
        message: String,
    ) -> Result<(), anyhow::Error> {
        debug!("📊 Status message: {}", message);
        
        if message == "pong" || message.to_lowercase().contains("heartbeat") {
            debug!("💓 Heartbeat acknowledged");
        }
        
        Ok(())
    }

    /// Handle typing indicator messages
    async fn handle_typing_message(
        &self,
        active: bool,
    ) -> Result<(), anyhow::Error> {
        debug!("⌨️ Typing indicator: {}", active);
        
        // Could forward to other connected clients in the future
        // For now, just acknowledge
        Ok(())
    }
}

/// Utility function to check if a message should use tools
/// This encapsulates the routing logic for reuse
pub fn should_use_tools(metadata: &Option<MessageMetadata>) -> bool {
    CONFIG.enable_chat_tools && metadata.is_some()
}

/// Utility function to extract file context from metadata
pub fn extract_file_context(metadata: &Option<MessageMetadata>) -> Option<String> {
    metadata.as_ref().and_then(|meta| {
        if let Some(file_path) = &meta.file_path {
            Some(format!("File: {}", file_path))
        } else if let Some(repo_id) = &meta.repo_id {
            Some(format!("Repository: {}", repo_id))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ws::message::MessageMetadata;

    #[test]
    fn test_should_use_tools() {
        // Test with metadata present
        let metadata_with_file = Some(MessageMetadata {
            file_path: Some("test.rs".to_string()),
            repo_id: None,
            attachment_id: None,
            language: None,
            selection: None,
        });
        
        // Test with no metadata
        let no_metadata = None;
        
        // Test with empty metadata
        let empty_metadata = Some(MessageMetadata {
            file_path: None,
            repo_id: None,
            attachment_id: None,
            language: None,
            selection: None,
        });
        
        // Note: These tests depend on CONFIG.enable_chat_tools
        // In a real test environment, we'd want to mock or set the config
        
        if CONFIG.enable_chat_tools {
            assert!(should_use_tools(&metadata_with_file));
            assert!(!should_use_tools(&no_metadata));
            assert!(should_use_tools(&empty_metadata)); // metadata exists even if empty
        } else {
            assert!(!should_use_tools(&metadata_with_file));
            assert!(!should_use_tools(&no_metadata));
            assert!(!should_use_tools(&empty_metadata));
        }
    }

    #[test]
    fn test_extract_file_context() {
        // Test with file path
        let metadata_with_file = Some(MessageMetadata {
            file_path: Some("src/main.rs".to_string()),
            repo_id: None,
            attachment_id: None,
            language: None,
            selection: None,
        });
        
        assert_eq!(extract_file_context(&metadata_with_file), Some("File: src/main.rs".to_string()));
        
        // Test with repo ID
        let metadata_with_repo = Some(MessageMetadata {
            file_path: None,
            repo_id: Some("repo-123".to_string()),
            attachment_id: None,
            language: None,
            selection: None,
        });
        
        assert_eq!(extract_file_context(&metadata_with_repo), Some("Repository: repo-123".to_string()));
        
        // Test with no metadata
        assert_eq!(extract_file_context(&None), None);
    }
}
