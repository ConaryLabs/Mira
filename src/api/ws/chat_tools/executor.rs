// src/api/ws/chat_tools/executor.rs
// Manages the execution of tools and streaming responses for WebSocket chat.

use std::sync::Arc;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tracing::{info, debug, error};
use futures::{Stream, StreamExt};

use crate::api::ws::message::MessageMetadata;
use crate::llm::responses::{
    types::{Message as ResponseMessage},
    ResponsesManager,
    ImageGenerationManager,
    ImageOptions,
};
use crate::memory::recall::RecallContext;
use crate::services::{chat_with_tools::get_enabled_tools, FileSearchService, FileSearchParams};
use crate::state::AppState;
use crate::config::CONFIG;

/// Configuration for the ToolExecutor, derived from the global application config.
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
    /// Creates a default tool configuration from the global CONFIG.
    fn default() -> Self {
        Self {
            enable_tools: CONFIG.enable_chat_tools,
            max_tools: 10, // Default value, can be configured if needed.
            tool_timeout_secs: 120, // Default value.
            model: CONFIG.gpt5_model.clone(),
            max_output_tokens: CONFIG.max_output_tokens,
            reasoning_effort: CONFIG.reasoning_effort.clone(),
        }
    }
}

impl ToolConfig {
    /// Overrides the model in the current configuration.
    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
    
    /// Overrides the max output tokens in the current configuration.
    pub fn with_max_output_tokens(mut self, tokens: usize) -> Self {
        self.max_output_tokens = tokens;
        self
    }
    
    /// Overrides the tool enablement setting in the current configuration.
    pub fn with_tools_enabled(mut self, enabled: bool) -> Self {
        self.enable_tools = enabled;
        self
    }
    
    /// Overrides the reasoning effort in the current configuration.
    pub fn with_reasoning_effort(mut self, effort: String) -> Self {
        self.reasoning_effort = effort;
        self
    }
}

/// Represents a request to execute a tool-enabled chat turn.
#[derive(Debug, Clone)]
pub struct ToolChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
    pub context: RecallContext,
    pub system_prompt: String,
}

/// Represents the final response after a tool-enabled chat turn.
#[derive(Debug, Clone)]
pub struct ToolChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCallResult>,
    pub metadata: Option<ResponseMetadata>,
}

/// Encapsulates the result of a single tool call.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub tool_type: String,
    pub tool_id: String,
    pub status: ToolCallStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
}

/// Represents the execution status of a tool.
#[derive(Debug, Clone)]
pub enum ToolCallStatus {
    Started,
    Completed,
    Failed,
}

/// Contains metadata about the final chat response.
#[derive(Debug, Clone)]
pub struct ResponseMetadata {
    pub mood: Option<String>,
    pub salience: Option<f32>,
    pub tags: Option<Vec<String>>,
    pub response_id: Option<String>,
}

/// Events representing the different states of a streaming, tool-enabled response.
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
    ImageGenerated {
        urls: Vec<String>,
        revised_prompt: Option<String>,
    },
    Complete {
        metadata: Option<ResponseMetadata>,
    },
    Error(String),
    Done,
}

/// Manages the logic for handling tool calls within a chat session.
pub struct ToolExecutor {
    responses_manager: Arc<ResponsesManager>,
    pub image_generation_manager: Option<Arc<ImageGenerationManager>>,
    pub file_search_service: Option<Arc<FileSearchService>>,
    config: ToolConfig,
}

impl ToolExecutor {
    /// Creates a new tool executor with default settings.
    pub fn new(responses_manager: Arc<ResponsesManager>) -> Self {
        info!("Initializing ToolExecutor with model: {}", CONFIG.gpt5_model);
        Self {
            responses_manager,
            image_generation_manager: None,
            file_search_service: None,
            config: ToolConfig::default(),
        }
    }

    /// Creates a new tool executor with all managers from the application state.
    pub fn from_app_state(app_state: &Arc<AppState>) -> Self {
        info!("Initializing ToolExecutor with all managers from AppState");
        Self {
            responses_manager: app_state.responses_manager.clone(),
            image_generation_manager: Some(app_state.image_generation_manager.clone()),
            file_search_service: Some(app_state.file_search_service.clone()),
            config: ToolConfig::default(),
        }
    }

    /// Creates a tool executor with a custom configuration and all managers.
    pub fn from_app_state_with_config(app_state: &Arc<AppState>, config: ToolConfig) -> Self {
        info!(
            "Initializing ToolExecutor with custom config - model: {}, tools_enabled: {}",
            config.model, config.enable_tools
        );
        Self {
            responses_manager: app_state.responses_manager.clone(),
            image_generation_manager: Some(app_state.image_generation_manager.clone()),
            file_search_service: Some(app_state.file_search_service.clone()),
            config,
        }
    }

    /// Creates a tool executor with a custom configuration (basic setup).
    pub fn with_config(responses_manager: Arc<ResponsesManager>, config: ToolConfig) -> Self {
        info!(
            "Initializing ToolExecutor with custom config - model: {}, tools_enabled: {}",
            config.model, config.enable_tools
        );
        Self {
            responses_manager,
            image_generation_manager: None,
            file_search_service: None,
            config,
        }
    }

    /// Sets the image generation manager for the executor.
    pub fn with_image_generation_manager(mut self, manager: Arc<ImageGenerationManager>) -> Self {
        self.image_generation_manager = Some(manager);
        self
    }

    /// Sets the file search service for the executor.
    pub fn with_file_search_service(mut self, service: Arc<FileSearchService>) -> Self {
        self.file_search_service = Some(service);
        self
    }

    /// Updates the executor's configuration at runtime.
    pub fn update_config(&mut self, config: ToolConfig) {
        info!("Updating ToolExecutor config - model: {}, tools_enabled: {}", config.model, config.enable_tools);
        self.config = config;
    }

    /// Checks if tools are enabled both locally and globally.
    pub fn tools_enabled(&self) -> bool {
        self.config.enable_tools && CONFIG.enable_chat_tools
    }

    /// Gets the current model from the configuration.
    pub fn get_model(&self) -> &str {
        &self.config.model
    }

    /// Gets a reference to the current configuration.
    pub fn get_config(&self) -> &ToolConfig {
        &self.config
    }

    /// Executes a tool-enabled chat request and returns a stream of events.
    pub async fn stream_with_tools<'a>(
        &'a self,
        request: &'a ToolChatRequest,
    ) -> Result<impl Stream<Item = ToolEvent> + 'a> {
        info!("Starting tool-enabled streaming chat for session: {}", request.session_id);

        let messages = vec![
            ResponseMessage {
                role: "system".to_string(),
                content: Some(request.system_prompt.clone()),
                ..Default::default()
            },
            ResponseMessage {
                role: "user".to_string(),
                content: Some(request.content.clone()),
                ..Default::default()
            },
        ];

        let tools = if self.tools_enabled() {
            Some(get_enabled_tools())
        } else {
            None
        };
        
        let mut request_body = json!({
            "model": self.config.model,
            "input": messages,
            "stream": true,
        });

        if let Some(tools) = tools {
            request_body["tools"] = json!(tools);
        }

        match self.responses_manager.create_streaming_response(
            &self.config.model,
            request_body["input"].as_array().unwrap().iter().map(|v| serde_json::from_value(v.clone()).unwrap()).collect(),
            Some(request.system_prompt.clone()),
            Some(&request.session_id),
            Some(request_body),
        ).await {
            Ok(stream) => {
                let tool_stream = stream.map(|chunk| {
                    match chunk {
                        Ok(stream_event) => {
                            match stream_event.get("type").and_then(|t| t.as_str()) {
                                Some("content_chunk") => {
                                    let content = stream_event.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
                                    ToolEvent::ContentChunk(content)
                                }
                                Some("tool_call_started") => {
                                    let tool_type = stream_event.get("tool_type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
                                    let tool_id = stream_event.get("tool_id").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
                                    ToolEvent::ToolCallStarted { tool_type, tool_id }
                                }
                                Some("tool_call_complete") => {
                                    let tool_type = stream_event.get("tool_type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
                                    let tool_id = stream_event.get("tool_id").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
                                    let result = stream_event.get("result").cloned().unwrap_or(serde_json::Value::Null);
                                    ToolEvent::ToolCallCompleted { tool_type, tool_id, result }
                                }
                                Some("tool_call_failed") => {
                                    let tool_type = stream_event.get("tool_type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
                                    let tool_id = stream_event.get("tool_id").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
                                    let error = stream_event.get("error").and_then(|e| e.as_str()).unwrap_or("Unknown error").to_string();
                                    ToolEvent::ToolCallFailed { tool_type, tool_id, error }
                                }
                                Some("complete") => ToolEvent::Complete { metadata: None }, // Metadata can be added here
                                Some("done") => ToolEvent::Done,
                                _ => ToolEvent::Error("Unknown stream event type".to_string()),
                            }
                        }
                        Err(e) => {
                            error!("Streaming error: {}", e);
                            ToolEvent::Error(e.to_string())
                        }
                    }
                });
                Ok(tool_stream.boxed())
            }
            Err(e) => {
                error!("ResponsesManager streaming API call failed: {}", e);
                Err(anyhow!("Tool streaming failed: {}", e))
            }
        }
    }

    /// Executes the image generation tool.
    pub async fn execute_image_generation(
        &self,
        prompt: &str,
        style: Option<String>,
        quality: Option<String>,
        size: Option<String>,
    ) -> Result<Value> {
        info!("Executing image generation tool for prompt: '{}'", prompt);
        
        let image_manager = self.image_generation_manager
            .as_ref()
            .ok_or_else(|| anyhow!("ImageGenerationManager not available"))?;
        
        let options = ImageOptions {
            n: Some(1),
            size: size.or_else(|| Some(CONFIG.image_generation_size.clone())),
            quality: quality.or_else(|| Some(CONFIG.image_generation_quality.clone())),
            style: style.or_else(|| Some(CONFIG.image_generation_style.clone())),
        };
        
        options.validate()?;
        
        let response = image_manager.generate_images(prompt, options).await?;
        
        let urls: Vec<&str> = response.urls();
        let revised_prompt = response.images.first().and_then(|img| img.revised_prompt.as_deref());
        
        info!("Image generation completed: {} images created.", response.images.len());
        
        Ok(json!({
            "prompt": prompt,
            "urls": urls,
            "revised_prompt": revised_prompt,
            "image_count": response.images.len(),
            "tool_type": "image_generation",
            "status": "completed"
        }))
    }

    /// Executes the file search tool.
    pub async fn execute_file_search(
        &self,
        query: &str,
        project_id: Option<&str>,
        file_extensions: Option<Vec<String>>,
        max_files: Option<usize>,
        case_sensitive: Option<bool>,
    ) -> Result<Value> {
        info!("Executing file search tool for query: '{}'", query);
        
        let file_search_service = self.file_search_service
            .as_ref()
            .ok_or_else(|| anyhow!("FileSearchService not available"))?;
        
        let params = FileSearchParams {
            query: query.to_string(),
            file_extensions,
            max_files,
            case_sensitive,
            include_content: Some(true),
        };
        
        let search_results = file_search_service
            .search_files(&params, project_id)
            .await?;
        
        info!("File search completed.");
        Ok(search_results)
    }

    /// Builds the list of messages for the API request from the chat request.
    fn build_messages(&self, request: &ToolChatRequest) -> Result<Vec<ResponseMessage>> {
        let mut messages = Vec::new();

        messages.push(ResponseMessage {
            role: "system".to_string(),
            content: Some(request.system_prompt.clone()),
            ..Default::default()
        });

        for recent_msg in &request.context.recent {
            messages.push(ResponseMessage {
                role: recent_msg.role.clone(),
                content: Some(recent_msg.content.clone()),
                ..Default::default()
            });
        }

        messages.push(ResponseMessage {
            role: "user".to_string(),
            content: Some(request.content.clone()),
            ..Default::default()
        });

        Ok(messages)
    }

    /// Executes a tool-enabled chat request in a non-streaming manner.
    pub async fn execute_with_tools(&self, request: &ToolChatRequest) -> Result<ToolChatResponse> {
        info!(
            "Executing non-streaming tool chat with model: {}",
            self.config.model,
        );

        let messages = self.build_messages(request)?;
        let tools = get_enabled_tools();
        debug!("Calling ResponsesManager with {} tools", tools.len());

        match self.responses_manager.create_response(
            &self.config.model,
            messages,
            None, None, None,
        ).await {
            Ok(api_response) => {
                debug!("Received response from ResponsesManager");
                let response = ToolChatResponse {
                    content: api_response,
                    tool_calls: Vec::new(), // Non-streaming response does not support tool calls directly here.
                    metadata: None, // Metadata would need to be parsed from the response if available.
                };
                info!("Tool execution completed successfully.");
                Ok(response)
            }
            Err(e) => {
                error!("ResponsesManager API call failed: {}", e);
                Err(anyhow!("Tool execution failed: {}", e))
            }
        }
    }

    /// Gets statistics about the current executor state.
    pub fn get_stats(&self) -> Value {
        json!({
            "model": self.config.model,
            "tools_enabled": self.tools_enabled(),
            "max_output_tokens": self.config.max_output_tokens,
            "reasoning_effort": self.config.reasoning_effort,
            "managers": {
                "responses_manager": true,
                "image_generation_manager": self.image_generation_manager.is_some(),
                "file_search_service": self.file_search_service.is_some(),
            },
            "available_tools": get_enabled_tools().iter().map(|t| &t.tool_type).collect::<Vec<_>>(),
        })
    }

    /// Validates the current configuration of the executor.
    pub fn validate_configuration(&self) -> Result<()> {
        if self.config.enable_tools && !CONFIG.enable_chat_tools {
            return Err(anyhow!("Tools are enabled in executor config but disabled globally"));
        }
        if self.config.max_output_tokens == 0 {
            return Err(anyhow!("Max output tokens cannot be zero"));
        }
        if self.config.model.is_empty() {
            return Err(anyhow!("Model name cannot be empty"));
        }
        Ok(())
    }
}
