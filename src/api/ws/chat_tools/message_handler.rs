// src/api/ws/chat_tools/message_handler.rs
// Phase 2: Extract Message Handling from chat_tools.rs
// Handles WebSocket-specific tool message processing and streaming

use std::sync::Arc;

use anyhow::Result;
use axum::extract::ws::Message;
use futures::StreamExt;
use futures_util::SinkExt; // For send() method
use tracing::{error, info, warn}; // Removed unused imports: debug, json, Mutex

use crate::api::ws::connection::WebSocketConnection;
use crate::api::ws::message::MessageMetadata;
use crate::api::ws::chat_tools::executor::{ToolExecutor, ToolChatRequest, ToolEvent}; // FIXED: Updated path
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
        _context: RecallContext, // Added underscore prefix for unused variable
        system_prompt: String,
        _session_id: String, // Added underscore prefix for unused variable
    ) -> Result<()> {
        info!("🔧 Handling tool message for content: {}", content.chars().take(50).collect::<String>());

        // Send initial status
        self.send_status("Initializing response...", Some("Setting up tools and context")).await?;

        // Check if tools are available
        if !self.executor.tools_enabled() {
            warn!("Tools are not enabled, falling back to simple response");
            return self.handle_simple_response(content).await;
        }

        // Create tool chat request
        let request = ToolChatRequest {
            content,
            project_id,
            metadata,
            session_id: "temp-session".to_string(), // Using temporary session ID
            context: RecallContext { recent: vec![], semantic: vec![] }, // Using empty context for now
            system_prompt,
        };

        // Execute with tools and stream results
        match self.executor.stream_with_tools(&request).await {
            Ok(mut stream) => {
                while let Some(event) = stream.next().await {
                    match event {
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
                            self.send_complete(
                                metadata.as_ref().and_then(|m| m.mood.clone()),
                                metadata.as_ref().and_then(|m| m.salience),
                                metadata.as_ref().and_then(|m| m.tags.clone()),
                            ).await?;
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
        let message = WsServerMessageWithTools::Chunk {
            content: content.to_string(),
            mood,
        };
        self.send_ws_message(message).await
    }

    async fn send_status(&self, message: &str, detail: Option<&str>) -> Result<()> {
        let message = WsServerMessageWithTools::Status {
            message: message.to_string(),
            detail: detail.map(|s| s.to_string()),
        };
        self.send_ws_message(message).await
    }

    async fn send_complete(
        &self,
        mood: Option<String>,
        salience: Option<f32>,
        tags: Option<Vec<String>>,
    ) -> Result<()> {
        let message = WsServerMessageWithTools::Complete {
            mood,
            salience,
            tags,
        };
        self.send_ws_message(message).await
    }

    async fn send_tool_call(&self, tool_type: &str, tool_id: &str, status: &str) -> Result<()> {
        let message = WsServerMessageWithTools::ToolCall {
            tool_type: tool_type.to_string(),
            tool_id: tool_id.to_string(),
            status: status.to_string(),
        };
        self.send_ws_message(message).await
    }

    async fn send_tool_result(&self, tool_type: &str, tool_id: &str, data: serde_json::Value) -> Result<()> {
        let message = WsServerMessageWithTools::ToolResult {
            tool_type: tool_type.to_string(),
            tool_id: tool_id.to_string(),
            data,
        };
        self.send_ws_message(message).await
    }

    async fn send_error(&self, message: &str, code: Option<&str>) -> Result<()> {
        let message = WsServerMessageWithTools::Error {
            message: message.to_string(),
            code: code.map(|s| s.to_string()),
        };
        self.send_ws_message(message).await
    }

    async fn send_done(&self) -> Result<()> {
        let message = WsServerMessageWithTools::Done;
        self.send_ws_message(message).await
    }

    async fn send_ws_message(&self, message: WsServerMessageWithTools) -> Result<()> {
        let json = serde_json::to_string(&message)?;
        self.connection.send_text(&json).await.map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))
    }

    async fn handle_simple_response(&self, content: String) -> Result<()> {
        // Fallback to simple response without tools
        self.send_chunk(&format!("Simple response for: {}", content), Some("neutral".to_string())).await?;
        self.send_complete(Some("neutral".to_string()), Some(5.0), Some(vec!["simple".to_string()])).await?;
        self.send_done().await
    }
}
