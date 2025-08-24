// src/api/ws/chat_tools/executor.rs
// PHASE 3 UPDATE: Added ImageGenerationManager and FileSearchService integration
// ENHANCED: Proper config management with setters and builder pattern

use std::sync::Arc;
use anyhow::Result;
use serde_json::{json, Value};
use tracing::{info, debug, error};
use futures::{Stream, StreamExt};

use crate::api::ws::message::MessageMetadata;
use crate::llm::responses::{
    types::{Message as ResponseMessage},
    ResponsesManager,
    ImageGenerationManager, // PHASE 3 NEW
    ImageOptions, // PHASE 3 NEW
};
use crate::memory::recall::RecallContext;
use crate::services::{chat_with_tools::get_enabled_tools, FileSearchService, FileSearchParams}; // PHASE 3: Added FileSearchService
use crate::state::AppState; // PHASE 3 NEW
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

impl ToolConfig {
    /// Create config with custom model
    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
    
    /// Create config with custom token limit
    pub fn with_max_output_tokens(mut self, tokens: usize) -> Self {
        self.max_output_tokens = tokens;
        self
    }
    
    /// Create config with tools enabled/disabled
    pub fn with_tools_enabled(mut self, enabled: bool) -> Self {
        self.enable_tools = enabled;
        self
    }
    
    /// Create config with custom reasoning effort
    pub fn with_reasoning_effort(mut self, effort: String) -> Self {
        self.reasoning_effort = effort;
        self
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
    // PHASE 3 NEW: Image generation event
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

/// ENHANCED: Tool executor with proper config management and Phase 3 integration
pub struct ToolExecutor {
    responses_manager: Arc<ResponsesManager>,
    image_generation_manager: Option<Arc<ImageGenerationManager>>, // PHASE 3 NEW
    file_search_service: Option<Arc<FileSearchService>>, // PHASE 3 NEW
    config: ToolConfig,
}

impl ToolExecutor {
    /// Create new tool executor with CONFIG-based defaults
    pub fn new(responses_manager: Arc<ResponsesManager>) -> Self {
        info!("Initializing ToolExecutor with model: {} (from CONFIG)", CONFIG.model);

        Self {
            responses_manager,
            image_generation_manager: None,
            file_search_service: None,
            config: ToolConfig::default(),
        }
    }

    /// PHASE 3 NEW: Create new tool executor from AppState with all managers
    pub fn from_app_state(app_state: &Arc<AppState>) -> Self {
        info!("Initializing ToolExecutor with all managers from AppState");

        Self {
            responses_manager: app_state.responses_manager.clone(),
            image_generation_manager: Some(app_state.image_generation_manager.clone()), // PHASE 3 NEW
            file_search_service: Some(app_state.file_search_service.clone()), // PHASE 3 NEW
            config: ToolConfig::default(),
        }
    }

    /// ENHANCED: Create tool executor with custom config and AppState managers
    pub fn from_app_state_with_config(app_state: &Arc<AppState>, config: ToolConfig) -> Self {
        info!(
            "Initializing ToolExecutor with custom config and full manager integration - model: {}, tools_enabled: {}",
            config.model, config.enable_tools
        );

        Self {
            responses_manager: app_state.responses_manager.clone(),
            image_generation_manager: Some(app_state.image_generation_manager.clone()),
            file_search_service: Some(app_state.file_search_service.clone()),
            config,
        }
    }

    /// Create tool executor with custom config (basic - no Phase 3 managers)
    pub fn with_config(responses_manager: Arc<ResponsesManager>, config: ToolConfig) -> Self {
        info!(
            "Initializing ToolExecutor with custom config - model: {}, tools_enabled: {}",
            config.model, config.enable_tools
        );

        Self {
            responses_manager,
            image_generation_manager: None, // Will need to be set separately
            file_search_service: None, // Will need to be set separately
            config,
        }
    }

    /// ENHANCED: Set image generation manager (for flexibility)
    pub fn with_image_generation_manager(mut self, manager: Arc<ImageGenerationManager>) -> Self {
        self.image_generation_manager = Some(manager);
        self
    }

    /// ENHANCED: Set file search service (for flexibility)
    pub fn with_file_search_service(mut self, service: Arc<FileSearchService>) -> Self {
        self.file_search_service = Some(service);
        self
    }

    /// ENHANCED: Update configuration (for runtime config changes)
    pub fn update_config(&mut self, config: ToolConfig) {
        info!("Updating ToolExecutor config - model: {}, tools_enabled: {}", config.model, config.enable_tools);
        self.config = config;
    }

    /// Check if tools are enabled
    pub fn tools_enabled(&self) -> bool {
        self.config.enable_tools && CONFIG.enable_chat_tools
    }

    /// Get current model from configuration
    pub fn get_model(&self) -> &str {
        &self.config.model
    }

    /// Get current configuration
    pub fn get_config(&self) -> &ToolConfig {
        &self.config
    }

    /// Execute chat with tools using streaming
    pub async fn stream_with_tools(
        &self,
        request: &ToolChatRequest,
    ) -> Result<impl Stream<Item = ToolEvent> + '_> {
        info!("🔧 Starting tool-enabled streaming chat");

        let messages = vec![
            ResponseMessage {
                role: "system".to_string(),
                content: Some(request.system_prompt.clone()),
                name: None,
                function_call: None,
                tool_calls: None,
            },
            ResponseMessage {
                role: "user".to_string(),
                content: Some(request.content.clone()),
                name: None,
                function_call: None,
                tool_calls: None,
            },
        ];

        let tools = if self.tools_enabled() {
            Some(get_enabled_tools())
        } else {
            None
        };

        // Create streaming request
        match self.responses_manager.stream_with_tools(
            &self.config.model,
            messages,
            tools,
            Some(&request.system_prompt),
            Some(self.config.max_output_tokens),
            None, // temperature
        ).await {
            Ok(stream) => {
                let tool_stream = stream.map(move |chunk| {
                    match chunk {
                        Ok(stream_event) => {
                            // Parse the streaming event and convert to ToolEvent
                            match stream_event.get("type").and_then(|t| t.as_str()) {
                                Some("content_chunk") => {
                                    let content = stream_event.get("content")
                                        .and_then(|c| c.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    
                                    ToolEvent::ContentChunk(content)
                                }
                                Some("tool_call_started") => {
                                    let tool_type = stream_event.get("tool_type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let tool_id = stream_event.get("tool_id")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    
                                }
                                Some("tool_call_complete") => {
                                    let tool_type = stream_event.get("tool_type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let tool_id = stream_event.get("tool_id")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let result = stream_event.get("result")
                                        .cloned()
                                        .unwrap_or(serde_json::Value::Null);
                                    
                                    ToolEvent::ToolCallCompleted { tool_type, tool_id, result }
                                }
                                Some("tool_call_failed") => {
                                    let tool_type = stream_event.get("tool_type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let tool_id = stream_event.get("tool_id")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let error = stream_event.get("error")
                                        .and_then(|e| e.as_str())
                                        .unwrap_or("Unknown error")
                                        .to_string();
                                    
                                    ToolEvent::ToolCallFailed { tool_type, tool_id, error }
                                }
                                Some("complete") => {
                                    ToolEvent::Complete { 
                                        metadata: Some(ResponseMetadata {
                                            mood: None,
                                            salience: None,
                                            tags: None,
                                            response_id: None,
                                        })
                                    }
                                }
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

                Ok(tool_stream)
            }
            Err(e) => {
                error!("ResponsesManager streaming API call failed: {}", e);
                Err(anyhow::anyhow!("Tool streaming failed: {}", e))
            }
        }
    }

    /// PHASE 3 NEW: Execute image generation tool
    pub async fn execute_image_generation(
        &self,
        prompt: &str,
        style: Option<String>,
        quality: Option<String>,
        size: Option<String>,
    ) -> Result<Value> {
        info!("🎨 Executing image generation tool");
        
        let image_manager = self.image_generation_manager
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ImageGenerationManager not available"))?;
        
        // Create options from parameters using CONFIG defaults
        let options = ImageOptions {
            n: Some(1),
            size: size.or_else(|| Some(CONFIG.image_generation_size.clone())),
            quality: quality.or_else(|| Some(CONFIG.image_generation_quality.clone())),
            style: style.or_else(|| Some(CONFIG.image_generation_style.clone())),
        };
        
        // Validate options
        options.validate()?;
        
        // Generate the image
        let response = image_manager.generate_images(prompt, options).await?;
        
        // Format response for tool system
        let urls: Vec<&str> = response.urls();
        let revised_prompt = response.images.first()
            .and_then(|img| img.revised_prompt.as_deref());
        
        info!("✅ Image generation completed: {} images", response.images.len());
        
        Ok(json!({
            "prompt": prompt,
            "urls": urls,
            "revised_prompt": revised_prompt,
            "image_count": response.images.len(),
            "tool_type": "image_generation",
            "status": "completed"
        }))
    }

    /// PHASE 3 NEW: Execute file search tool
    pub async fn execute_file_search(
        &self,
        query: &str,
        project_id: Option<&str>,
        file_extensions: Option<Vec<String>>,
        max_files: Option<usize>,
        case_sensitive: Option<bool>,
    ) -> Result<Value> {
        info!("🔍 Executing file search tool for query: '{}'", query);
        
        let file_search_service = self.file_search_service
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("FileSearchService not available"))?;
        
        // Create search parameters
        let params = FileSearchParams {
            query: query.to_string(),
            file_extensions,
            max_files,
            case_sensitive,
            include_content: Some(true),
        };
        
        // Execute the search
        let search_results = file_search_service
            .search_files(&params, project_id)
            .await?;
        
        info!("✅ File search completed");
        Ok(search_results)
    }

    /// Build messages from request
    fn build_messages(&self, request: &ToolChatRequest) -> Result<Vec<ResponseMessage>> {
        let mut messages = Vec::new();

        // Add system prompt
        messages.push(ResponseMessage {
            role: "system".to_string(),
            content: Some(request.system_prompt.clone()),
            name: None,
            function_call: None,
            tool_calls: None,
        });

        // Add recent context if available
        for recent_msg in &request.context.recent {
            messages.push(ResponseMessage {
                role: recent_msg.role.clone(),
                content: Some(recent_msg.content.clone()),
                name: None,
                function_call: None,
                tool_calls: None,
            });
        }

        // Add user message
        messages.push(ResponseMessage {
            role: "user".to_string(),
            content: Some(request.content.clone()),
            name: None,
            function_call: None,
            tool_calls: None,
        });

        Ok(messages)
    }

    /// Execute chat with tools (non-streaming) - for compatibility
    pub async fn execute_with_tools(&self, request: &ToolChatRequest) -> Result<ToolChatResponse> {
        info!(
            "Executing with tools using model: {} for content: {}",
            self.config.model,
            request.content.chars().take(50).collect::<String>()
        );

        let messages = self.build_messages(request)?;
        let tools = get_enabled_tools();

        debug!("Calling ResponsesManager with {} tools", tools.len());

        // Call the actual ResponsesManager API with correct parameters
        match self.responses_manager.create_response(
            &self.config.model,
            messages,
            None, // instructions
            None, // response_format
            None, // parameters
        ).await {
            Ok(api_response) => {
                debug!("Received response from ResponsesManager");

                // The response is a String, so we need to handle it accordingly
                let tool_calls = Vec::new(); // No tool calls in basic response
                let metadata = Some(ResponseMetadata {
                    mood: None,
                    salience: None,
                    tags: None,
                    response_id: None,
                });

                info!("Tool execution completed");

                Ok(ToolChatResponse {
                    content: api_response,
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

    /// ENHANCED: Get executor statistics
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

    /// ENHANCED: Validate executor configuration
    pub fn validate_configuration(&self) -> Result<()> {
        if self.config.enable_tools && !CONFIG.enable_chat_tools {
            return Err(anyhow::anyhow!("Tools are enabled in config but disabled globally"));
        }

        if self.config.max_output_tokens == 0 {
            return Err(anyhow::anyhow!("Max output tokens cannot be zero"));
        }

        if self.config.model.is_empty() {
            return Err(anyhow::anyhow!("Model name cannot be empty"));
        }

        Ok(())
    }
}
