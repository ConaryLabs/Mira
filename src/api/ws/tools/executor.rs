// src/api/ws/tools/executor.rs
// Phase 1: Extract Tool Execution Logic from chat_tools.rs
// Handles tool execution using ResponsesManager with streaming support

use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use tracing::info;

use crate::api::ws::message::MessageMetadata;
use crate::llm::responses::types::Message as ResponseMessage;
use crate::llm::responses::ResponsesManager;
use crate::memory::recall::RecallContext;

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

/// Tool executor with ResponsesManager integration
pub struct ToolExecutor {
    responses_manager: Arc<ResponsesManager>,
    config: ToolConfig,
}

impl ToolExecutor {
    /// Create new tool executor with default config
    pub fn new(responses_manager: Arc<ResponsesManager>) -> Self {
        Self {
            responses_manager,
            config: ToolConfig::default(),
        }
    }

    /// Create tool executor with custom config
    pub fn with_config(responses_manager: Arc<ResponsesManager>, config: ToolConfig) -> Self {
        Self {
            responses_manager,
            config,
        }
    }

    /// Check if tools are enabled
    pub fn tools_enabled(&self) -> bool {
        self.config.enable_tools
    }

    /// Execute chat with tools (non-streaming)
    pub async fn execute_with_tools(&self, request: &ToolChatRequest) -> Result<ToolChatResponse> {
        info!("🔧 Executing with tools for content: {}", request.content.chars().take(50).collect::<String>());

        // Build messages for the responses API
        let messages = self.build_messages(request)?;

        // Create streaming response with correct parameters according to the actual method signature:
        // pub async fn create_streaming_response(
        //     &self,
        //     model: &str,
        //     input: Vec<Message>,
        //     instructions: Option<String>,
        //     session_id: Option<&str>,
        //     parameters: Option<Value>,
        // )
        let _stream_result = self.responses_manager.create_streaming_response(
            "gpt-5",                                                    // &str (model)
            messages,                                                   // Vec<Message>
            Some(request.system_prompt.clone()),                       // Option<String> (instructions)
            Some(&request.session_id),                                 // Option<&str> (session_id)
            None,                                                      // Option<Value> (parameters)
        ).await;

        // Mock tool calls for now
        let tool_calls: Vec<ToolCallResult> = Vec::new();

        Ok(ToolChatResponse {
            content: format!("Tool response for: {}", request.content),
            tool_calls,
            metadata: Some(ResponseMetadata {
                mood: Some("helpful".to_string()),
                salience: Some(7.0),
                tags: Some(vec!["tool-response".to_string()]),
                response_id: None,
            }),
        })
    }

    /// Stream chat with tools
    pub async fn stream_with_tools(&self, request: &ToolChatRequest) -> Result<impl futures::Stream<Item = ToolEvent>> {
        info!("🔧 Streaming with tools for content: {}", request.content.chars().take(50).collect::<String>());

        // Build messages for the responses API
        let _messages = self.build_messages(request)?;

        // For now, create a simple mock stream
        let _content = String::new();
        let _tool_calls: Vec<ToolCallResult> = Vec::new();
        let _metadata: Option<ResponseMetadata> = None;

        // Create a simple stream that yields some mock events
        let events = vec![
            ToolEvent::ContentChunk("Processing your request...".to_string()),
            ToolEvent::Complete {
                metadata: Some(ResponseMetadata {
                    mood: Some("helpful".to_string()),
                    salience: Some(7.0),
                    tags: Some(vec!["tool-response".to_string()]),
                    response_id: None,
                }),
            },
            ToolEvent::Done,
        ];

        // Convert to stream
        let tool_stream = futures::stream::iter(events.into_iter().map(|event| event));

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
                user_content = format!("File: {}\n\n{}", file_path, user_content);
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
}
