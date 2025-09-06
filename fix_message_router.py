#!/usr/bin/env python3
"""
Completely rewrite message_router.rs to fix the mangled file
"""

import os
from pathlib import Path
import argparse
import sys

def create_fixed_message_router():
    """Create a properly formatted message_router.rs"""
    return '''// src/api/ws/chat/message_router.rs
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
            // New WebSocket-only command handlers
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
            debug!("Received heartbeat response from client");
        }
        
        Ok(())
    }

    /// Handle typing indicator
    async fn handle_typing_message(
        &self,
        active: bool,
    ) -> Result<(), anyhow::Error> {
        debug!("Typing indicator: {}", if active { "active" } else { "inactive" });
        // Could broadcast to other clients in a multi-user scenario
        Ok(())
    }
    
    // Stub handlers for new message types (to be implemented in Phase 2-6)
    async fn handle_project_command(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        info!("Project command: {} with params: {:?}", method, params);
        // Phase 4: Implement project operations
        self.connection.send_error("Project commands not yet implemented", "NOT_IMPLEMENTED".to_string()).await?;
        Ok(())
    }

    async fn handle_memory_command(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        info!("Memory command: {} with params: {:?}", method, params);
        // Phase 2: Implement memory operations
        self.connection.send_error("Memory commands not yet implemented", "NOT_IMPLEMENTED".to_string()).await?;
        Ok(())
    }

    async fn handle_git_command(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        info!("Git command: {} with params: {:?}", method, params);
        // Phase 5: Implement git operations
        self.connection.send_error("Git commands not yet implemented", "NOT_IMPLEMENTED".to_string()).await?;
        Ok(())
    }

    async fn handle_file_transfer(
        &self,
        operation: String,
        data: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        info!("File transfer: {} with data: {:?}", operation, data);
        // Phase 6: Implement file transfers
        self.connection.send_error("File transfers not yet implemented", "NOT_IMPLEMENTED".to_string()).await?;
        Ok(())
    }
}

/// Determine if tools should be used based on metadata
/// PHASE 3 FIX: Extracted to a standalone function for clarity
pub fn should_use_tools(metadata: &Option<MessageMetadata>) -> bool {
    // Check if metadata indicates tool usage should be enabled
    if let Some(meta) = metadata {
        // You can add more sophisticated logic here based on metadata fields
        return true;
    }
    // Default to checking CONFIG
    CONFIG.enable_chat_tools
}

/// Extract file context from metadata if present
pub fn extract_file_context(metadata: &Option<MessageMetadata>) -> Option<String> {
    // Extract file context from metadata if present
    if let Some(meta) = metadata {
        // You can extract file context from metadata here
        // For now, return None
        return None;
    }
    None
}
'''

def main():
    parser = argparse.ArgumentParser(description='Fix message_router.rs completely')
    parser.add_argument('backend_path', help='Path to the Mira backend directory')
    parser.add_argument('--execute', action='store_true', 
                       help='Actually execute the changes (default is dry-run)')
    
    args = parser.parse_args()
    
    backend_path = Path(args.backend_path)
    router_path = backend_path / "src" / "api" / "ws" / "chat" / "message_router.rs"
    
    if not args.execute:
        print("\n⚠️  DRY RUN MODE - No changes will be made")
        print("Add --execute flag to actually perform the fix\n")
        print(f"Will rewrite: {router_path}")
        return
    
    print(f"Rewriting {router_path}...")
    
    # Backup the current file
    if router_path.exists():
        backup_path = router_path.with_suffix('.rs.backup')
        with open(router_path, 'r') as f:
            backup_content = f.read()
        with open(backup_path, 'w') as f:
            f.write(backup_content)
        print(f"Backed up current file to {backup_path}")
    
    # Write the fixed content
    with open(router_path, 'w') as f:
        f.write(create_fixed_message_router())
    
    print("✅ message_router.rs has been completely rewritten")
    print("\nNext: Run 'cargo build' to check if it compiles")

if __name__ == "__main__":
    main()
