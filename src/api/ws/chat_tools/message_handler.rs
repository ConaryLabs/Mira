// src/api/ws/chat_tools/message_handler.rs
// Phase 2: Extract Message Handling from chat_tools.rs
// Handles WebSocket-specific tool message processing and streaming

use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use tracing::{error, info, warn};

use crate::api::ws::connection::WebSocketConnection;
use crate::api::ws::message::{MessageMetadata, WsServerMessage};
use crate::api::ws::chat_tools::executor::{ToolExecutor, ToolChatRequest, ToolEvent};
use crate::memory::recall::RecallContext;
use crate::state::AppState;

/// Enhanced WebSocket server messages with tool support
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum WsServerMessageWithTools {
    // Existing message types
    Chunk { 
        content: String, 
        mood: Option<String> 
    },
    Complete { 
        mood: Option<String>, 
        salience: Option<f32>, 
        tags: Option<Vec<String>> 
    },
    Status { 
        message: String, 
        detail: Option<String> 
    },
    Aside { 
        emotional_cue: String, 
        intensity: Option<f32> 
    },
    Error { 
        message: String,
        code: Option<String>
    },
    Done,
    
    // Tool-related message types (for UI feedback)
    ToolCall {
        tool_type: String,
        tool_id: String,
        status: String, // "started", "completed", "failed"
    },
    ToolResult { 
        tool_type: String,
        tool_id: String,
        data: serde_json::Value,
    },
}

/// Tool message handler for WebSocket communication
pub struct ToolMessageHandler {
    executor: Arc<ToolExecutor>,
    connection: Arc<WebSocketConnection>,
    app_state: Arc<AppState>,
}

impl ToolMessageHandler {
    pub fn new(
        executor: Arc<ToolExecutor>,
        connection: Arc<WebSocketConnection>,
        app_state: Arc<AppState>,
    ) -> Self {
        Self {
            executor,
            connection,
            app_state,
        }
    }

    /// Handle tool-enabled chat message with streaming
    pub async fn handle_tool_message(
        &self,
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
        context: RecallContext, // Use the context parameter
        system_prompt: String,
        session_id: String, // Use the session_id parameter
    ) -> Result<()> {
        info!("🔧 Handling tool message for content: {}", content.chars().take(50).collect::<String>());

        // Send initial status
        self.send_status("Initializing response...", Some("Setting up tools and context")).await?;

        // Check if tools are available
        if !self.executor.tools_enabled() {
            warn!("Tools are not enabled, falling back to simple response");
            return self.handle_simple_response(content).await;
        }

        // Create tool chat request with all required fields
        let request = ToolChatRequest {
            content,
            project_id,
            metadata,
            session_id,        // Add the required session_id field
            context,           // Add the required context field
            system_prompt,
        };

        // Execute with tools and stream results using the correct method and response type
        match self.executor.stream_with_tools(&request).await {
            Ok(mut stream) => {
                while let Some(event) = stream.next().await {
                    match event {
                        // Match the actual ToolEvent variants from executor.rs
                        ToolEvent::ContentChunk(chunk) => {
                            self.send_chunk(&chunk, None).await?;
                        }
                        ToolEvent::ToolCallStarted { tool_type, tool_id } => {
                            self.send_tool_call(&tool_type, &tool_id, "started").await?;
                        }
                        ToolEvent::ToolCallCompleted { tool_type, tool_id, result } => {
                            self.send_tool_call(&tool_type, &tool_id, "completed").await?;
                            self.send_tool_result(&tool_type, &tool_id, result).await?;
                        }
                        ToolEvent::ToolCallFailed { tool_type, tool_id, error } => {
                            self.send_tool_call(&tool_type, &tool_id, "failed").await?;
                            self.send_error(&format!("Tool {} failed: {}", tool_type, error), None).await?;
                        }
                        ToolEvent::Complete { metadata } => {
                            // Extract fields from metadata properly
                            let mood = metadata.as_ref().and_then(|m| m.mood.clone());
                            let salience = metadata.as_ref().and_then(|m| m.salience);
                            let tags = metadata.as_ref().and_then(|m| m.tags.clone());
                            self.send_complete(mood, salience, tags).await?;
                        }
                        ToolEvent::Error(error_msg) => {
                            self.send_error(&error_msg, None).await?;
                        }
                        ToolEvent::Done => {
                            self.send_done().await?;
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to execute with tools: {}", e);
                self.send_error(&format!("Failed to process with tools: {}", e), None).await?;
            }
        }

        Ok(())
    }

    // Helper methods for sending WebSocket messages
    async fn send_chunk(&self, content: &str, mood: Option<String>) -> Result<()> {
        let message = WsServerMessage::Chunk {
            content: content.to_string(),
            mood,
        };
        self.connection.send_message(message).await.map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))
    }

    async fn send_status(&self, message: &str, detail: Option<&str>) -> Result<()> {
        // Use the existing send_status method from WebSocketConnection
        if let Some(detail) = detail {
            self.connection.send_status(&format!("{} - {}", message, detail)).await.map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))
        } else {
            self.connection.send_status(message).await.map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))
        }
    }

    async fn send_complete(
        &self,
        mood: Option<String>,
        salience: Option<f32>,
        tags: Option<Vec<String>>,
    ) -> Result<()> {
        let message = WsServerMessage::Complete {
            mood,
            salience,
            tags,
        };
        self.connection.send_message(message).await.map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))
    }

    async fn send_tool_call(&self, tool_type: &str, tool_id: &str, status: &str) -> Result<()> {
        // Send as status message since we don't have a specific tool call message type in WsServerMessage
        let status_msg = format!("Tool {}: {} - {}", tool_type, tool_id, status);
        self.connection.send_status(&status_msg).await.map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))
    }

    async fn send_tool_result(&self, tool_type: &str, tool_id: &str, data: serde_json::Value) -> Result<()> {
        // Send tool result as status message with JSON data
        let result_msg = format!("Tool result {}: {} - {}", tool_type, tool_id, data);
        self.connection.send_status(&result_msg).await.map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))
    }

    async fn send_error(&self, message: &str, _code: Option<&str>) -> Result<()> {
        // Use the existing send_error method from WebSocketConnection
        self.connection.send_error(message).await.map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))
    }

    async fn send_done(&self) -> Result<()> {
        let message = WsServerMessage::Done;
        self.connection.send_message(message).await.map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))
    }

    async fn handle_simple_response(&self, content: String) -> Result<()> {
        // Fallback to simple response without tools
        self.send_chunk(&format!("Simple response for: {}", content), Some("neutral".to_string())).await?;
        self.send_complete(Some("neutral".to_string()), Some(5.0), Some(vec!["simple".to_string()])).await?;
        self.send_done().await
    }
}
