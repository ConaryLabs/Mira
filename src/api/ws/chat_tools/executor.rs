// src/api/ws/chat_tools/executor.rs
// Complete tool integration with real ResponsesManager API calls

use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;
use tracing::{info, debug, warn, error};
use futures::{Stream, StreamExt};
use tokio_stream::wrappers::ReceiverStream;

use crate::api::ws::message::MessageMetadata;
use crate::llm::responses::{
    types::{Message as ResponseMessage, Tool, CreateStreamingResponse},
    ResponsesManager,
};
use crate::memory::recall::RecallContext;
use crate::services::chat_with_tools::get_enabled_tools;
use crate::config::CONFIG;

/// Configuration for tool execution using centralized CONFIG
#[derive(Debug, Clone)]
pub struct ToolConfig {
    pub enable_tools: bool,
    pub max_tools: usize,
    pub tool_timeout_secs: u64,
    pub model: String,
    pub max_output_tokens: usize,
    pub reasoning_effort: String,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            enable_tools: CONFIG.enable_chat_tools,
            max_tools: 10,
            tool_timeout_secs: 120,
            model: CONFIG.model.clone(),
            max_output_tokens: CONFIG.max_output_tokens,
            reasoning_effort: CONFIG.reasoning_effort.clone(),
        }
    }
}

/// Tool execution request
#[derive(Debug, Clone)]
pub struct ToolChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
    pub context: RecallContext,
    pub system_prompt: String,
}

/// Tool execution response
#[derive(Debug, Clone)]
pub struct ToolChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCallResult>,
    pub metadata: Option<ResponseMetadata>,
}

/// Result of a tool call
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub tool_type: String,
    pub tool_id: String,
    pub status: ToolCallStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
}

/// Status of a tool call
#[derive(Debug, Clone)]
pub enum ToolCallStatus {
    Started,
    Completed,
    Failed,
}

/// Response metadata
#[derive(Debug, Clone)]
pub struct ResponseMetadata {
    pub mood: Option<String>,
    pub salience: Option<f32>,
    pub tags: Option<Vec<String>>,
    pub response_id: Option<String>,
}

/// Tool execution events for streaming
#[derive(Debug, Clone)]
pub enum ToolEvent {
    ContentChunk(String),
    ToolCallStarted {
        tool_type: String,
        tool_id: String,
    },
    ToolCallCompleted {
        tool_type: String,
        tool_id: String,
        result: Value,
    },
    ToolCallFailed {
        tool_type: String,
        tool_id: String,
        error: String,
    },
    Complete {
        metadata: Option<ResponseMetadata>,
    },
    Error(String),
    Done,
}

/// Tool executor with complete ResponsesManager integration
pub struct ToolExecutor {
    responses_manager: Arc<ResponsesManager>,
    config: ToolConfig,
}

impl ToolExecutor {
    /// Create new tool executor with CONFIG-based defaults
    pub fn new(responses_manager: Arc<ResponsesManager>) -> Self {
        info!("Initializing ToolExecutor with model: {} (from CONFIG)", CONFIG.model);

        Self {
            responses_manager,
            config: ToolConfig::default(),
        }
    }

    /// Create tool executor with custom config
    pub fn with_config(responses_manager: Arc<ResponsesManager>, config: ToolConfig) -> Self {
        info!(
            "Initializing ToolExecutor with custom config - model: {}, tools_enabled: {}",
            config.model, config.enable_tools
        );

        Self {
            responses_manager,
            config,
        }
    }

    /// Check if tools are enabled
    pub fn tools_enabled(&self) -> bool {
        self.config.enable_tools && CONFIG.enable_chat_tools
    }

    /// Get current model from configuration
    pub fn get_model(&self) -> &str {
        &self.config.model
    }

    /// Execute chat with tools (non-streaming) - COMPLETED IMPLEMENTATION
    pub async fn execute_with_tools(&self, request: &ToolChatRequest) -> Result<ToolChatResponse> {
        info!(
            "Executing with tools using model: {} for content: {}",
            self.config.model,
            request.content.chars().take(50).collect::<String>()
        );

        let messages = self.build_messages(request)?;
        let tools = get_enabled_tools();

        // Create the streaming response request
        let create_request = CreateStreamingResponse {
            messages,
            tools: Some(tools),
            model: Some(self.config.model.clone()),
            system_prompt: Some(request.system_prompt.clone()),
            max_output_tokens: Some(self.config.max_output_tokens),
            temperature: Some(0.7),
            stream: false, // Non-streaming for this method
        };

        debug!("Calling ResponsesManager with {} tools", create_request.tools.as_ref().unwrap().len());

        // Call the actual ResponsesManager API
        match self.responses_manager.create_response(&create_request).await {
            Ok(api_response) => {
                debug!("Received response from ResponsesManager");

                // Extract tool calls from the response
                let mut tool_calls = Vec::new();
                if let Some(tool_call_results) = &api_response.tool_calls {
                    for tool_call in tool_call_results {
                        tool_calls.push(ToolCallResult {
                            tool_type: tool_call.tool_type.clone(),
                            tool_id: tool_call.tool_id.clone().unwrap_or_else(|| "unknown".to_string()),
                            status: if tool_call.status == "completed" {
                                ToolCallStatus::Completed
                            } else if tool_call.status == "failed" {
                                ToolCallStatus::Failed
                            } else {
                                ToolCallStatus::Started
                            },
                            result: tool_call.result.clone(),
                            error: tool_call.error.clone(),
                        });
                    }
                }

                // Extract metadata
                let metadata = Some(ResponseMetadata {
                    mood: api_response.mood,
                    salience: api_response.salience,
                    tags: api_response.tags,
                    response_id: api_response.response_id,
                });

                info!("Tool execution completed with {} tool calls", tool_calls.len());

                Ok(ToolChatResponse {
                    content: api_response.content,
                    tool_calls,
                    metadata,
                })
            }
            Err(e) => {
                error!("ResponsesManager API call failed: {}", e);
                Err(anyhow::anyhow!("Tool execution failed: {}", e))
            }
        }
    }

    /// Execute chat with tools (streaming) - COMPLETED IMPLEMENTATION
    pub async fn stream_with_tools(&self, request: &ToolChatRequest) -> Result<impl Stream<Item = ToolEvent>> {
        info!("Starting streaming tool execution with model: {}", self.config.model);

        let messages = self.build_messages(request)?;
        let tools = get_enabled_tools();

        // Create the streaming response request
        let create_request = CreateStreamingResponse {
            messages,
            tools: Some(tools),
            model: Some(self.config.model.clone()),
            system_prompt: Some(request.system_prompt.clone()),
            max_output_tokens: Some(self.config.max_output_tokens),
            temperature: Some(0.7),
            stream: true, // Enable streaming
        };

        debug!("Starting streaming response with {} tools", create_request.tools.as_ref().unwrap().len());

        // Call the actual ResponsesManager streaming API
        match self.responses_manager.create_streaming_response(&create_request).await {
            Ok(response_stream) => {
                info!("Streaming response initiated successfully");

                // Convert ResponsesManager stream events to ToolEvent
                let tool_stream = response_stream.map(|event_result| {
                    match event_result {
                        Ok(stream_event) => {
                            // Convert ResponsesManager events to ToolEvent
                            match stream_event.event_type.as_str() {
                                "text_delta" => {
                                    if let Some(text) = stream_event.data.get("text").and_then(|t| t.as_str()) {
                                        ToolEvent::ContentChunk(text.to_string())
                                    } else {
                                        ToolEvent::Error("Invalid text delta event".to_string())
                                    }
                                }
                                "tool_call_start" => {
                                    let tool_type = stream_event.data.get("tool_type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let tool_id = stream_event.data.get("tool_id")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    
                                    ToolEvent::ToolCallStarted { tool_type, tool_id }
                                }
                                "tool_call_complete" => {
                                    let tool_type = stream_event.data.get("tool_type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let tool_id = stream_event.data.get("tool_id")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let result = stream_event.data.get("result")
                                        .cloned()
                                        .unwrap_or_else(|| Value::Null);
                                    
                                    ToolEvent::ToolCallCompleted { tool_type, tool_id, result }
                                }
                                "tool_call_error" => {
                                    let tool_type = stream_event.data.get("tool_type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let tool_id = stream_event.data.get("tool_id")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let error = stream_event.data.get("error")
                                        .and_then(|e| e.as_str())
                                        .unwrap_or("Unknown error")
                                        .to_string();
                                    
                                    ToolEvent::ToolCallFailed { tool_type, tool_id, error }
                                }
                                "response_complete" => {
                                    let metadata = Some(ResponseMetadata {
                                        mood: stream_event.data.get("mood").and_then(|m| m.as_str()).map(String::from),
                                        salience: stream_event.data.get("salience").and_then(|s| s.as_f64()).map(|f| f as f32),
                                        tags: stream_event.data.get("tags")
                                            .and_then(|t| t.as_array())
                                            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()),
                                        response_id: stream_event.data.get("response_id").and_then(|r| r.as_str()).map(String::from),
                                    });
                                    
                                    ToolEvent::Complete { metadata }
                                }
                                "done" => ToolEvent::Done,
                                "error" => {
                                    let error_msg = stream_event.data.get("message")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("Unknown streaming error")
                                        .to_string();
                                    
                                    ToolEvent::Error(error_msg)
                                }
                                _ => {
                                    debug!("Unknown stream event type: {}", stream_event.event_type);
                                    ToolEvent::Error(format!("Unknown event type: {}", stream_event.event_type))
                                }
                            }
                        }
                        Err(e) => {
                            error!("Stream error: {}", e);
                            ToolEvent::Error(format!("Stream error: {}", e))
                        }
                    }
                });

                Ok(tool_stream)
            }
            Err(e) => {
                error!("Failed to create streaming response: {}", e);
                Err(anyhow::anyhow!("Streaming tool execution failed: {}", e))
            }
        }
    }

    /// Build messages for ResponsesManager API
    fn build_messages(&self, request: &ToolChatRequest) -> Result<Vec<ResponseMessage>> {
        debug!("Building messages for tool execution");

        let mut messages = Vec::new();

        // Add context messages from recent history
        for msg in &request.context.recent {
            messages.push(ResponseMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            });
        }

        // Add current user message
        messages.push(ResponseMessage {
            role: "user".to_string(),
            content: request.content.clone(),
        });

        debug!("Built {} messages for tool execution", messages.len());
        Ok(messages)
    }

    /// Get tool configuration summary for debugging
    pub fn get_config_summary(&self) -> String {
        format!(
            "ToolExecutor Config: model={}, tools_enabled={}, max_tools={}, timeout={}s",
            self.config.model,
            self.config.enable_tools,
            self.config.max_tools,
            self.config.tool_timeout_secs
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_config_default() {
        let config = ToolConfig::default();
        assert_eq!(config.model, CONFIG.model);
        assert_eq!(config.max_output_tokens, CONFIG.max_output_tokens);
        assert_eq!(config.reasoning_effort, CONFIG.reasoning_effort);
    }

    #[test]
    fn test_tool_call_status() {
        let result = ToolCallResult {
            tool_type: "web_search".to_string(),
            tool_id: "search_1".to_string(),
            status: ToolCallStatus::Completed,
            result: Some(serde_json::json!({"url": "https://example.com"})),
            error: None,
        };
        
        assert!(matches!(result.status, ToolCallStatus::Completed));
        assert!(result.result.is_some());
        assert!(result.error.is_none());
    }
}
