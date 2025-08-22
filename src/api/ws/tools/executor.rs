// src/api/ws/tools/executor.rs
// Phase 1: Extract Tool Execution Logic from chat_tools.rs
// Handles tool execution using ResponsesManager with streaming support

use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::llm::responses::types::{Message as ResponseMessage, Tool};
use crate::llm::responses::ResponsesManager;
use crate::memory::recall::RecallContext;
use crate::state::AppState;

/// Configuration for tool execution
#[derive(Debug, Clone)]
pub struct ToolConfig {
    pub enable_tools: bool,
    pub max_tools: usize,
    pub tool_timeout_secs: u64,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            enable_tools: true,
            max_tools: 10,
            tool_timeout_secs: 120,
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
    ContentDelta(String),
    ToolCallStarted { tool_type: String, tool_id: String },
    ToolCallCompleted { tool_type: String, tool_id: String, result: Value },
    ToolCallFailed { tool_type: String, tool_id: String, error: String },
    MetadataExtracted(ResponseMetadata),
    Done,
    Error(String),
}

/// Tool executor that manages tool execution using ResponsesManager
pub struct ToolExecutor {
    responses_manager: Arc<ResponsesManager>,
    config: ToolConfig,
}

impl ToolExecutor {
    pub fn new(responses_manager: Arc<ResponsesManager>) -> Self {
        Self {
            responses_manager,
            config: ToolConfig::default(),
        }
    }

    pub fn with_config(responses_manager: Arc<ResponsesManager>, config: ToolConfig) -> Self {
        Self {
            responses_manager,
            config,
        }
    }

    /// Execute tools with streaming response
    pub async fn execute_with_tools(&self, request: &ToolChatRequest) -> Result<ToolChatResponse> {
        info!("🔧 Executing tools for session: {}", request.session_id);

        // Build messages for the Responses API
        let messages = self.build_messages(request)?;
        
        // Get enabled tools
        let tools = crate::services::chat_with_tools::get_enabled_tools();
        info!("🔧 {} tools available", tools.len());

        if tools.is_empty() {
            return Err(anyhow::anyhow!("No tools available for execution"));
        }

        // Execute via ResponsesManager
        let model = "gpt-5"; // Could be configurable
        let stream_result = self.responses_manager.create_streaming_response(
            model,
            messages,
            None, // instructions handled via system prompt
            Some(&request.session_id),
            Some(json!({
                "tools": tools,
                "tool_choice": "auto",
                "stream": false, // Non-streaming for this method
                "verbosity": "medium",
                "reasoning_effort": "medium",
                "max_output_tokens": 128000,
            })),
        ).await?;

        // Process the response (this is a simplified version for non-streaming)
        // In practice, the streaming version would be used more often
        let mut content = String::new();
        let mut tool_calls = Vec::new();
        let mut metadata = None;

        // For now, we'll create a placeholder response
        // The actual implementation would process the stream results
        Ok(ToolChatResponse {
            content,
            tool_calls,
            metadata,
        })
    }

    /// Stream tool execution with real-time events
    pub async fn stream_with_tools(&self, request: &ToolChatRequest) -> Result<impl futures::Stream<Item = Result<ToolEvent>> + Send> {
        info!("🚀 Starting streaming tool execution for session: {}", request.session_id);

        // Build messages for the Responses API
        let messages = self.build_messages(request)?;
        
        // Get enabled tools
        let tools = crate::services::chat_with_tools::get_enabled_tools();
        info!("🔧 {} tools available for streaming", tools.len());

        if tools.is_empty() {
            return Err(anyhow::anyhow!("No tools available for streaming execution"));
        }

        // Create streaming response
        let model = "gpt-5"; // Could be configurable
        let stream = self.responses_manager.create_streaming_response(
            model,
            messages,
            None, // instructions handled via system prompt
            Some(&request.session_id),
            Some(json!({
                "tools": tools,
                "tool_choice": "auto",
                "stream": true,
                "verbosity": "medium",
                "reasoning_effort": "medium",
                "max_output_tokens": 128000,
            })),
        ).await?;

        // Transform the stream into ToolEvent stream
        let tool_stream = stream.map(|chunk_result| {
            match chunk_result {
                Ok(event) => {
                    // Parse different event types
                    if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                        match event_type {
                            "response.text.delta" => {
                                if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                    Ok(ToolEvent::ContentDelta(delta.to_string()))
                                } else {
                                    Ok(ToolEvent::Error("Invalid text delta format".to_string()))
                                }
                            },
                            "response.tool_call.started" => {
                                let tool_type = event.get("tool_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                let tool_id = event.get("tool_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                Ok(ToolEvent::ToolCallStarted { tool_type, tool_id })
                            },
                            "response.tool_call.completed" => {
                                let tool_type = event.get("tool_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                let tool_id = event.get("tool_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                let result = event.get("result").cloned().unwrap_or(json!({}));
                                Ok(ToolEvent::ToolCallCompleted { tool_type, tool_id, result })
                            },
                            "response.tool_call.failed" => {
                                let tool_type = event.get("tool_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                let tool_id = event.get("tool_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                let error = event.get("error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Unknown error")
                                    .to_string();
                                Ok(ToolEvent::ToolCallFailed { tool_type, tool_id, error })
                            },
                            "response.done" => {
                                Ok(ToolEvent::Done)
                            },
                            _ => {
                                debug!("Unhandled event type: {}", event_type);
                                Ok(ToolEvent::Error(format!("Unhandled event type: {}", event_type)))
                            }
                        }
                    } else {
                        Ok(ToolEvent::Error("Missing event type".to_string()))
                    }
                },
                Err(e) => Ok(ToolEvent::Error(format!("Stream error: {}", e))),
            }
        });

        Ok(tool_stream)
    }

    /// Build messages for the Responses API
    fn build_messages(&self, request: &ToolChatRequest) -> Result<Vec<ResponseMessage>> {
        let mut messages = vec![];
        
        // Add system message
        messages.push(ResponseMessage {
            role: "system".to_string(),
            content: Some(request.system_prompt.clone()),
            name: None,
            function_call: None,
            tool_calls: None,
        });
        
        // Add recent context as assistant/user messages
        for entry in request.context.recent.iter().take(10) {
            messages.push(ResponseMessage {
                role: entry.role.clone(),
                content: Some(entry.content.clone()),
                name: None,
                function_call: None,
                tool_calls: None,
            });
        }
        
        // Add current user message with any file context
        let mut user_content = request.content.clone();
        if let Some(meta) = &request.metadata {
            if let Some(file_path) = &meta.file_path {
                user_content = format!("[File: {}]\n{}", file_path, user_content);
            }
        }
        
        messages.push(ResponseMessage {
            role: "user".to_string(),
            content: Some(user_content),
            name: None,
            function_call: None,
            tool_calls: None,
        });
        
        Ok(messages)
    }

    /// Get available tools
    pub fn get_available_tools(&self) -> Vec<Tool> {
        crate::services::chat_with_tools::get_enabled_tools()
    }

    /// Check if tools are enabled
    pub fn tools_enabled(&self) -> bool {
        self.config.enable_tools && !self.get_available_tools().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::client::OpenAIClient;
    use std::sync::Arc;

    #[test]
    fn test_tool_config_default() {
        let config = ToolConfig::default();
        assert!(config.enable_tools);
        assert_eq!(config.max_tools, 10);
        assert_eq!(config.tool_timeout_secs, 120);
    }

    #[tokio::test]
    async fn test_tool_executor_creation() {
        // This would require a real OpenAI client for integration testing
        // For now, we'll test the basic structure
        std::env::set_var("OPENAI_API_KEY", "test-key");
        let client = OpenAIClient::new().unwrap();
        let responses_manager = Arc::new(ResponsesManager::new(client));
        
        let executor = ToolExecutor::new(responses_manager);
        assert!(executor.config.enable_tools);
    }

    #[test]
    fn test_tool_event_variants() {
        let event = ToolEvent::ContentDelta("Hello".to_string());
        match event {
            ToolEvent::ContentDelta(content) => assert_eq!(content, "Hello"),
            _ => panic!("Expected ContentDelta"),
        }
        
        let event = ToolEvent::ToolCallStarted {
            tool_type: "search".to_string(),
            tool_id: "call_123".to_string(),
        };
        match event {
            ToolEvent::ToolCallStarted { tool_type, tool_id } => {
                assert_eq!(tool_type, "search");
                assert_eq!(tool_id, "call_123");
            },
            _ => panic!("Expected ToolCallStarted"),
        }
    }
}
