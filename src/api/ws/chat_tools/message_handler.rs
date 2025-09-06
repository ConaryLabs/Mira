// src/api/ws/chat_tools/message_handler.rs
// Handles WebSocket message processing and streaming for tool-enabled chat.

use std::sync::Arc;
use anyhow::Result;
use futures_util::StreamExt;
use tracing::{error, info, warn};

use crate::api::ws::chat::connection::WebSocketConnection;
use crate::api::ws::message::{MessageMetadata, WsServerMessage};
use crate::api::ws::chat_tools::executor::{ToolExecutor, ToolChatRequest, ToolEvent};
use crate::memory::recall::RecallContext;
use crate::state::AppState;

/// Handles the logic for processing tool-related messages over a WebSocket connection.
pub struct ToolMessageHandler {
    executor: Arc<ToolExecutor>,
    connection: Arc<WebSocketConnection>,
}

impl ToolMessageHandler {
    pub fn new(
        executor: Arc<ToolExecutor>,
        connection: Arc<WebSocketConnection>,
        _app_state: Arc<AppState>,
    ) -> Self {
        Self { executor, connection }
    }

    /// Handles a tool-enabled chat message and streams the response events to the client.
    pub async fn handle_tool_message(
        &self,
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
        context: RecallContext,
        system_prompt: String,
        session_id: String,
    ) -> Result<()> {
        info!("Handling tool message for session {}: {}", session_id, content.chars().take(80).collect::<String>());

        // FIX: The send_status method only takes one argument.
        self.connection.send_status("Initializing response...", None).await?;

        if !self.executor.tools_enabled() {
            warn!("Tools are not enabled, falling back to a simple response.");
            return self.handle_simple_response(content).await;
        }

        let request = ToolChatRequest { content, project_id, metadata, session_id, context, system_prompt };

        match self.executor.stream_with_tools(&request).await {
            Ok(mut stream) => {
                while let Some(event) = stream.next().await {
                    match event {
                        ToolEvent::ContentChunk(chunk) => {
                            self.connection.send_message(WsServerMessage::StreamChunk { text: chunk }).await?;
                        }
                        ToolEvent::ToolCallStarted { tool_type, tool_id } => {
                            let status_detail = format!("Tool '{}' ({}) started.", tool_type, tool_id);
                            self.connection.send_status("Executing tool...", Some(status_detail)).await?;
                        }
                        ToolEvent::ToolCallCompleted { tool_type, tool_id, result } => {
                            let result_str = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                            let status_detail = format!("Tool '{}' ({}) completed. Result: {}", tool_type, tool_id, result_str);
                            self.connection.send_status("Tool executed successfully.", Some(status_detail)).await?;
                        }
                        ToolEvent::ToolCallFailed { tool_type, tool_id, error } => {
                            let err_msg = format!("Tool '{}' ({}) failed: {}", tool_type, tool_id, error);
                            self.connection.send_error(&err_msg, "TOOL_FAILED".to_string()).await?;
                        }
                        ToolEvent::ImageGenerated { urls, revised_prompt } => {
                            info!("Image generated with {} URLs", urls.len());
                            self.connection.send_message(WsServerMessage::ImageGenerated { urls, revised_prompt }).await?;
                        }
                        ToolEvent::Complete { metadata } => {
                            let (mood, salience, tags) = if let Some(meta) = metadata { (meta.mood, meta.salience, meta.tags) } else { (None, None, None) };
                            self.connection.send_message(WsServerMessage::Complete { mood, salience, tags }).await?;
                        }
                        ToolEvent::Error(error_msg) => {
                            self.connection.send_error(&error_msg, "STREAM_ERROR".to_string()).await?;
                        }
                        ToolEvent::Done => {
                            self.connection.send_message(WsServerMessage::Done).await?;
                            break;
                        }
                    }
                }
                Ok(())
            }
            Err(e) => {
                let error_message = format!("Tool execution failed: {}", e);
                error!("{}", error_message);
                self.connection.send_error(&error_message, "TOOL_EXEC_ERROR".to_string()).await?;
                Err(e)
            }
        }
    }

    /// Fallback handler for sending a simple response when tools are disabled.
    async fn handle_simple_response(&self, content: String) -> Result<()> {
        info!("Handling simple response because tools are disabled.");
        let response_content = format!("Tools are currently disabled. You said: {}", content);
        self.connection.send_message(WsServerMessage::StreamChunk { text: response_content }).await?;
        self.connection.send_message(WsServerMessage::Complete { 
            mood: Some("informative".to_string()), 
            salience: Some(0.5), 
            tags: None 
        }).await?;
        self.connection.send_message(WsServerMessage::Done).await?;
        Ok(())
    }
}
