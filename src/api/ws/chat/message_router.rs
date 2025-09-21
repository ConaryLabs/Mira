// src/api/ws/chat/message_router.rs
// Routes incoming WebSocket messages to appropriate handlers.
// Updated for structured responses - no more streaming!

use std::sync::Arc;
use std::net::SocketAddr;

use tracing::{debug, error, info};

use super::connection::WebSocketConnection;
use super::unified_handler::{UnifiedChatHandler, ChatRequest};
use crate::api::ws::message::{WsClientMessage, WsServerMessage, MessageMetadata};
use crate::api::ws::{memory, project, git, files, filesystem, code_intelligence};
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
            WsClientMessage::CodeIntelligenceCommand { method, params } => {
                self.handle_code_intelligence_command(method, params, request_id).await
            }
        }
    }

    /// Routes all chat messages through the unified handler - NEW: Direct response handling
    async fn handle_chat_message(
        &self,
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
    ) -> Result<(), anyhow::Error> {
        info!("Chat message received: {} chars", content.len());
        
        // Set processing flag
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
        
        // NEW: Get complete response instead of stream
        let result = match self.unified_handler.handle_message(request).await {
            Ok(complete_response) => {
                // Send the complete response as WebSocket events
                self.send_complete_response_to_client(complete_response).await
            }
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
    
    /// NEW: Send complete response to client in expected WebSocket format
    async fn send_complete_response_to_client(
        &self,
        complete_response: crate::llm::structured::CompleteResponse,
    ) -> Result<(), anyhow::Error> {
        // Send the response content as a stream chunk (for frontend compatibility)
        self.connection.send_message(WsServerMessage::StreamChunk { 
            text: complete_response.structured.output.clone() 
        }).await?;
        
        // Send stream end
        self.connection.send_message(WsServerMessage::StreamEnd).await?;
        
        // Send complete with metadata (for frontend compatibility)
        self.connection.send_message(WsServerMessage::Complete {
            mood: complete_response.structured.analysis.mood,
            salience: Some(complete_response.structured.analysis.salience),
            tags: Some(complete_response.structured.analysis.topics),
        }).await?;
        
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
        debug!("Typing indicator: {}", if active { "active" } else { "inactive" });
        Ok(())
    }

    /// Handle project commands
    async fn handle_project_command(
        &self,
        method: String,
        params: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let response = project::handle_project_command(&self.app_state, method, params).await?;
        self.connection.send_message(WsServerMessage::Data { 
            data: response, 
            request_id 
        }).await?;
        Ok(())
    }

    /// Handle memory commands
    async fn handle_memory_command(
        &self,
        method: String,
        params: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let response = memory::handle_memory_command(&self.app_state, method, params).await?;
        self.connection.send_message(WsServerMessage::Data { 
            data: response, 
            request_id 
        }).await?;
        Ok(())
    }

    /// Handle git commands
    async fn handle_git_command(
        &self,
        method: String,
        params: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let response = git::handle_git_command(&self.app_state, method, params).await?;
        self.connection.send_message(WsServerMessage::Data { 
            data: response, 
            request_id 
        }).await?;
        Ok(())
    }

    /// Handle filesystem commands
    async fn handle_filesystem_command(
        &self,
        method: String,
        params: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let response = filesystem::handle_filesystem_command(&self.app_state, method, params).await?;
        self.connection.send_message(WsServerMessage::Data { 
            data: response, 
            request_id 
        }).await?;
        Ok(())
    }

    /// Handle file transfer
    async fn handle_file_transfer(
        &self,
        operation: String,
        data: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let response = files::handle_file_transfer(&self.app_state, operation, data).await?;
        self.connection.send_message(WsServerMessage::Data { 
            data: response, 
            request_id 
        }).await?;
        Ok(())
    }

    /// Handle code intelligence commands
    async fn handle_code_intelligence_command(
        &self,
        method: String,
        params: serde_json::Value,
        request_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let response = code_intelligence::handle_code_intelligence_command(&self.app_state, method, params).await?;
        self.connection.send_message(WsServerMessage::Data { 
            data: response, 
            request_id 
        }).await?;
        Ok(())
    }
}
