// src/api/ws/chat_tools/message_handler.rs
// Phase 2: Extract Message Handling from chat_tools.rs
// Handles WebSocket-specific tool message processing and streaming
// FIXED: Added missing ImageGenerated pattern match

use std::sync::Arc;

use anyhow::Result;
use futures_util::StreamExt; // Added for .next() method on streams
use tracing::{error, info, warn};

// FIXED: Updated import path - connection is now in chat/ directory
use crate::api::ws::chat::connection::WebSocketConnection;
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
    // PHASE 3 NEW: Image generation specific message type
    ImageGenerated {
        urls: Vec<String>,
        revised_prompt: Option<String>,
        tool_id: String,
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
                        // FIXED: Added missing ImageGenerated pattern match
                        ToolEvent::ImageGenerated { urls, revised_prompt } => {
                            info!("🎨 Image generated with {} URLs", urls.len());
                            self.send_image_generated(urls, revised_prompt, "img_gen_1".to_string()).await?;
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
                
                Ok(())
            }
            Err(e) => {
                error!("Tool execution failed: {}", e);
                self.send_error(&format!("Tool execution failed: {}", e), Some("TOOL_EXEC_ERROR".to_string())).await?;
                Err(e)
            }
        }
    }

    // Private helper methods for sending WebSocket messages

    async fn send_chunk(&self, content: &str, mood: Option<String>) -> Result<()> {
        let message = WsServerMessageWithTools::Chunk { 
            content: content.to_string(), 
            mood 
        };
        self.send_ws_message(message).await
    }

    async fn send_complete(
        &self, 
        mood: Option<String>, 
        salience: Option<f32>, 
        tags: Option<Vec<String>>
    ) -> Result<()> {
        let message = WsServerMessageWithTools::Complete { mood, salience, tags };
        self.send_ws_message(message).await
    }

    async fn send_status(&self, message: &str, detail: Option<&str>) -> Result<()> {
        let status_msg = WsServerMessageWithTools::Status { 
            message: message.to_string(), 
            detail: detail.map(|d| d.to_string()) 
        };
        self.send_ws_message(status_msg).await
    }

    async fn send_error(&self, message: &str, code: Option<String>) -> Result<()> {
        let error_msg = WsServerMessageWithTools::Error { 
            message: message.to_string(), 
            code 
        };
        self.send_ws_message(error_msg).await
    }

    async fn send_done(&self) -> Result<()> {
        let done_msg = WsServerMessageWithTools::Done;
        self.send_ws_message(done_msg).await
    }

    async fn send_tool_call(&self, tool_type: &str, tool_id: &str, status: &str) -> Result<()> {
        let tool_call_msg = WsServerMessageWithTools::ToolCall {
            tool_type: tool_type.to_string(),
            tool_id: tool_id.to_string(),
            status: status.to_string(),
        };
        self.send_ws_message(tool_call_msg).await
    }

    async fn send_tool_result(&self, tool_type: &str, tool_id: &str, data: serde_json::Value) -> Result<()> {
        let tool_result_msg = WsServerMessageWithTools::ToolResult {
            tool_type: tool_type.to_string(),
            tool_id: tool_id.to_string(),
            data,
        };
        self.send_ws_message(tool_result_msg).await
    }

    // PHASE 3 NEW: Send image generation result
    async fn send_image_generated(&self, urls: Vec<String>, revised_prompt: Option<String>, tool_id: String) -> Result<()> {
        let image_msg = WsServerMessageWithTools::ImageGenerated {
            urls,
            revised_prompt,
            tool_id,
        };
        self.send_ws_message(image_msg).await
    }

    async fn send_ws_message(&self, message: WsServerMessageWithTools) -> Result<()> {
        // Convert our custom message type to the expected WsServerMessage
        let ws_message = match message {
            WsServerMessageWithTools::Chunk { content, mood } => {
                WsServerMessage::Chunk { content, mood }
            }
            WsServerMessageWithTools::Complete { mood, salience, tags } => {
                WsServerMessage::Complete { mood, salience, tags }
            }
            WsServerMessageWithTools::Status { message, detail: _ } => {
                // Use send_status method instead
                return self.connection.send_status(&message).await;
            }
            WsServerMessageWithTools::Aside { emotional_cue, intensity: _ } => {
                // Convert to status message for aside
                let aside_msg = format!("Aside: {}", emotional_cue);
                return self.connection.send_status(&aside_msg).await;
            }
            WsServerMessageWithTools::Error { message, code: _ } => {
                WsServerMessage::Error { message, code: "TOOL_ERROR".to_string() }
            }
            WsServerMessageWithTools::Done => {
                WsServerMessage::Done
            }
            WsServerMessageWithTools::ToolCall { tool_type, tool_id, status } => {
                // Convert to status message
                let status_msg = format!("Tool {}: {} - {}", tool_type, tool_id, status);
                return self.connection.send_status(&status_msg).await;
            }
            WsServerMessageWithTools::ToolResult { tool_type, tool_id, data } => {
                // Convert to status message with JSON data
                let result_msg = format!("Tool result {}: {} - {}", tool_type, tool_id, data);
                return self.connection.send_status(&result_msg).await;
            }
            WsServerMessageWithTools::ImageGenerated { urls, revised_prompt, tool_id: _ } => {
                // Convert to status message with image URLs
                let image_msg = format!("Image generated: {} URLs{}", 
                    urls.len(),
                    if let Some(prompt) = revised_prompt { 
                        format!(" (revised: {})", prompt) 
                    } else { 
                        String::new() 
                    }
                );
                return self.connection.send_status(&image_msg).await;
            }
        };
        
        self.connection.send_message(ws_message).await
    }

    /// Fallback handler for simple responses when tools are not available
    async fn handle_simple_response(&self, content: String) -> Result<()> {
        info!("Handling simple response (tools disabled)");
        
        let response_content = format!("I received your message: {}", content);
        self.send_chunk(&response_content, Some("helpful".to_string())).await?;
        self.send_complete(Some("helpful".to_string()), Some(0.5), None).await?;
        self.send_done().await?;
        
        Ok(())
    }
}
