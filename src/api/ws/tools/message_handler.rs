// src/api/ws/tools/message_handler.rs
// Phase 2: Extract Message Handling from chat_tools.rs
// Handles WebSocket-specific tool message processing and streaming

use std::sync::Arc;

use anyhow::Result;
use axum::extract::ws::Message;
use futures::StreamExt;
use futures_util::stream::SplitSink;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::api::ws::connection::WebSocketConnection;
use crate::api::ws::message::MessageMetadata;
use crate::api::ws::tools::executor::{ToolExecutor, ToolChatRequest, ToolEvent};
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
        context: RecallContext,
        system_prompt: String,
        session_id: String,
    ) -> Result<()> {
        info!("🔧 Handling tool message for session: {}", session_id);

        // Send initial status
        self.send_status("Initializing response...", Some("Setting up tools and context")).await?;

        // Check if tools are available
        if !self.executor.tools_enabled() {
            warn!("No tools available, falling back to simple streaming");
            return self.handle_without_tools(content, context, system_prompt, session_id).await;
        }

        // Create tool chat request
        let request = ToolChatRequest {
            content,
            project_id,
            metadata,
            session_id,
            context,
            system_prompt,
        };

        // Start streaming with tools
        self.stream_with_tools(request).await
    }

    /// Stream response with tool support
    async fn stream_with_tools(&self, request: ToolChatRequest) -> Result<()> {
        info!("🚀 Starting tool-enabled streaming");

        // Get streaming tool events
        let mut stream = self.executor.stream_with_tools(&request).await?;
        
        let mut full_text = String::new();
        let mut chunks_sent = 0;
        let mut tool_calls = Vec::new();

        // Process the stream
        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    match event {
                        ToolEvent::ContentDelta(delta) => {
                            full_text.push_str(&delta);
                            chunks_sent += 1;
                            
                            let chunk_msg = WsServerMessageWithTools::Chunk {
                                content: delta,
                                mood: None,
                            };
                            
                            self.send_tool_message(chunk_msg).await?;
                        },
                        ToolEvent::ToolCallStarted { tool_type, tool_id } => {
                            info!("🔧 Tool call started: {} ({})", tool_type, tool_id);
                            
                            let tool_msg = WsServerMessageWithTools::ToolCall {
                                tool_type: tool_type.clone(),
                                tool_id: tool_id.clone(),
                                status: "started".to_string(),
                            };
                            
                            self.send_tool_message(tool_msg).await?;
                        },
                        ToolEvent::ToolCallCompleted { tool_type, tool_id, result } => {
                            info!("✅ Tool call completed: {} ({})", tool_type, tool_id);
                            
                            let tool_result_msg = WsServerMessageWithTools::ToolResult {
                                tool_type: tool_type.clone(),
                                tool_id: tool_id.clone(),
                                data: result.clone(),
                            };
                            
                            self.send_tool_message(tool_result_msg).await?;
                            
                            // Also send status update
                            let tool_status_msg = WsServerMessageWithTools::ToolCall {
                                tool_type,
                                tool_id,
                                status: "completed".to_string(),
                            };
                            
                            self.send_tool_message(tool_status_msg).await?;
                        },
                        ToolEvent::ToolCallFailed { tool_type, tool_id, error } => {
                            error!("❌ Tool call failed: {} ({}): {}", tool_type, tool_id, error);
                            
                            let tool_msg = WsServerMessageWithTools::ToolCall {
                                tool_type,
                                tool_id,
                                status: "failed".to_string(),
                            };
                            
                            self.send_tool_message(tool_msg).await?;
                        },
                        ToolEvent::MetadataExtracted(metadata) => {
                            debug!("📊 Metadata extracted: {:?}", metadata);
                            // Store metadata for completion message
                        },
                        ToolEvent::Done => {
                            info!("✅ Tool streaming complete: {} chunks, {} chars", 
                                 chunks_sent, full_text.len());
                            break;
                        },
                        ToolEvent::Error(error) => {
                            error!("❌ Tool event error: {}", error);
                            
                            let err_msg = WsServerMessageWithTools::Error {
                                message: format!("Tool error: {}", error),
                                code: Some("TOOL_ERROR".to_string()),
                            };
                            
                            self.send_tool_message(err_msg).await?;
                            break;
                        },
                    }
                },
                Err(e) => {
                    error!("❌ Stream error: {}", e);
                    
                    let err_msg = WsServerMessageWithTools::Error {
                        message: format!("Stream error: {}", e),
                        code: Some("STREAM_ERROR".to_string()),
                    };
                    
                    self.send_tool_message(err_msg).await?;
                    break;
                }
            }
        }

        // Finalize the response
        self.finalize_tool_response(full_text, request).await
    }

    /// Handle message without tools (fallback)
    async fn handle_without_tools(
        &self,
        content: String,
        context: RecallContext,
        system_prompt: String,
        session_id: String,
    ) -> Result<()> {
        info!("💬 Handling message without tools");

        // Use the simple streaming logic from the parent module
        use crate::llm::streaming::{start_response_stream, StreamEvent};
        
        let mut stream = start_response_stream(
            &self.app_state.llm_client,
            &content,
            Some(&system_prompt),
            false,
        ).await?;
        
        let mut full_text = String::new();
        let mut chunks_sent = 0;
        
        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::Delta(chunk)) => {
                    full_text.push_str(&chunk);
                    chunks_sent += 1;
                    
                    let chunk_msg = WsServerMessageWithTools::Chunk {
                        content: chunk,
                        mood: None,
                    };
                    
                    self.send_tool_message(chunk_msg).await?;
                }
                Ok(StreamEvent::Done { .. }) => {
                    info!("✅ Simple streaming complete: {} chunks, {} chars", 
                         chunks_sent, full_text.len());
                    break;
                }
                Ok(StreamEvent::Error(e)) => {
                    error!("Stream error: {}", e);
                    break;
                }
                Err(e) => {
                    error!("Stream decode error: {}", e);
                    break;
                }
            }
        }

        // Simple finalization without tools
        self.send_done().await
    }

    /// Send a tool message via WebSocket
    async fn send_tool_message(&self, message: WsServerMessageWithTools) -> Result<()> {
        let json_str = serde_json::to_string(&message)?;
        debug!("📤 Sending tool message: {} bytes", json_str.len());
        
        // Get the sender from connection and send
        let mut lock = self.connection.get_sender().lock().await;
        lock.send(Message::Text(json_str)).await?;
        
        Ok(())
    }

    /// Send status message
    async fn send_status(&self, message: &str, detail: Option<&str>) -> Result<()> {
        let status_msg = WsServerMessageWithTools::Status {
            message: message.to_string(),
            detail: detail.map(|s| s.to_string()),
        };
        
        self.send_tool_message(status_msg).await
    }

    /// Send error message
    async fn send_error(&self, message: &str, code: Option<&str>) -> Result<()> {
        let error_msg = WsServerMessageWithTools::Error {
            message: message.to_string(),
            code: code.map(|s| s.to_string()),
        };
        
        self.send_tool_message(error_msg).await
    }

    /// Send done message
    async fn send_done(&self) -> Result<()> {
        let done_msg = WsServerMessageWithTools::Done;
        self.send_tool_message(done_msg).await
    }

    /// Finalize tool response with metadata and memory save
    async fn finalize_tool_response(
        &self,
        full_text: String,
        request: ToolChatRequest,
    ) -> Result<()> {
        // Run metadata extraction (simplified for now)
        // In practice, this would use the metadata from ToolEvent::MetadataExtracted
        let mood = Some("helpful".to_string());
        let salience = Some(7.0);
        let tags = Some(vec!["tool-assisted".to_string()]);

        // Send completion message
        let complete_msg = WsServerMessageWithTools::Complete {
            mood: mood.clone(),
            salience,
            tags: tags.clone(),
        };

        self.send_tool_message(complete_msg).await?;

        // Save response to memory
        if let Err(e) = self.save_tool_response(&full_text, &request, mood, salience, tags).await {
            warn!("⚠️ Failed to save tool response: {}", e);
        }

        // Send final done message
        self.send_done().await
    }

    /// Save tool response to memory
    async fn save_tool_response(
        &self,
        full_text: &str,
        request: &ToolChatRequest,
        mood: Option<String>,
        salience: Option<f32>,
        tags: Option<Vec<String>>,
    ) -> Result<()> {
        use crate::services::chat::ChatResponse;
        use crate::config::CONFIG;

        let chat_response = ChatResponse {
            output: full_text.to_string(),
            persona: CONFIG.default_persona.clone(),
            mood: mood.unwrap_or_else(|| "helpful".to_string()),
            salience: salience.map(|s| s as usize).unwrap_or(7),
            summary: "".to_string(), // Tool responses don't need summaries
            memory_type: "tool_response".to_string(),
            tags: tags.unwrap_or_else(|| vec!["tool-assisted".to_string()]),
            intent: Some("tool_response".to_string()),
            monologue: None,
            reasoning_summary: None,
        };

        self.app_state
            .memory_service
            .save_assistant_response(&request.session_id, &chat_response)
            .await?;

        info!("💾 Saved tool response to memory");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_server_message_serialization() {
        let chunk_msg = WsServerMessageWithTools::Chunk {
            content: "Hello".to_string(),
            mood: None,
        };
        
        let json = serde_json::to_string(&chunk_msg).unwrap();
        assert!(json.contains("\"type\":\"Chunk\""));
        assert!(json.contains("\"content\":\"Hello\""));
    }

    #[test]
    fn test_tool_call_message() {
        let tool_msg = WsServerMessageWithTools::ToolCall {
            tool_type: "search".to_string(),
            tool_id: "call_123".to_string(),
            status: "started".to_string(),
        };
        
        let json = serde_json::to_string(&tool_msg).unwrap();
        assert!(json.contains("\"type\":\"ToolCall\""));
        assert!(json.contains("\"tool_type\":\"search\""));
        assert!(json.contains("\"status\":\"started\""));
    }

    #[test]
    fn test_tool_result_message() {
        let result_data = json!({"result": "search complete", "count": 5});
        let tool_result = WsServerMessageWithTools::ToolResult {
            tool_type: "search".to_string(),
            tool_id: "call_123".to_string(),
            data: result_data,
        };
        
        let json = serde_json::to_string(&tool_result).unwrap();
        assert!(json.contains("\"type\":\"ToolResult\""));
        assert!(json.contains("\"data\":{"));
    }
}
