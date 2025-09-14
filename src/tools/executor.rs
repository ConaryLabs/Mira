// src/tools/executor.rs
// Tool execution framework for coordinating tool-enabled responses.
// Manages the execution of various tools and streaming of results.

use std::pin::Pin;
use std::sync::Arc;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures_util::Stream;
use serde::Serialize;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::config::CONFIG;
use crate::memory::recall::RecallContext;
use crate::state::AppState;
use crate::utils::with_timeout;
use crate::llm::responses::image::{ImageOptions, ImageGenerationResponse};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ToolChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
    pub context: RecallContext,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum ToolEvent {
    ContentChunk(String),
    ToolExecution { tool_name: String, status: String },
    ToolResult { tool_name: String, result: serde_json::Value },
    ToolCallStarted { tool_type: String, tool_id: String },
    ToolCallCompleted { tool_type: String, tool_id: String, result: Value },
    ToolCallFailed { tool_type: String, tool_id: String, error: String },
    ImageGenerated { urls: Vec<String>, revised_prompt: Option<String> },
    Complete { metadata: Option<serde_json::Value> },
    Done,
    Error(String),
}

/// Primary tool executor that coordinates tool execution and response streaming
pub struct ToolExecutor;

impl ToolExecutor {
    pub fn new() -> Self {
        Self
    }

    /// Determines whether to use tool-enabled chat based on metadata and configuration
    pub fn should_use_tools(&self, metadata: &Option<MessageMetadata>) -> bool {
        if !CONFIG.enable_chat_tools {
            return false;
        }
        
        if let Some(meta) = metadata {
            // Use tools if we have file context, repository, or attachments
            if meta.file_path.is_some() || 
               meta.repo_id.is_some() || 
               meta.attachment_id.is_some() ||
               meta.language.is_some() {
                debug!("Using tools based on metadata context");
                return true;
            }
        }
        
        false
    }
    
    /// Extract context from uploaded files for enhanced responses
    pub fn extract_file_context(&self, metadata: &Option<MessageMetadata>) -> Option<String> {
        metadata.as_ref().and_then(|meta| {
            if let Some(file_path) = &meta.file_path {
                Some(format!("Working with file: {file_path}"))
            } else if let Some(repo_id) = &meta.repo_id {
                Some(format!("Repository context: {repo_id}"))
            } else { 
                meta.attachment_id.as_ref()
                    .map(|id| format!("Attachment: {id}"))
            }
        })
    }

    pub async fn stream_with_tools(
        &self,
        request: &ToolChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ToolEvent> + Send>>> {
        debug!("Starting tool-enabled stream for session: {}", request.session_id);

        if !CONFIG.enable_chat_tools {
            warn!("Tools disabled in config, returning error event");
            let event_stream = futures_util::stream::once(async move {
                ToolEvent::Error("Tools are not enabled".to_string())
            });
            return Ok(Box::pin(event_stream));
        }

        let content = request.content.clone();
        let session_id = request.session_id.clone();

        let event_stream = async_stream::stream! {
            info!("Processing request for session: {}", session_id);

            yield ToolEvent::ContentChunk(format!("Processing your request: {}", content.chars().take(50).collect::<String>()));

            // Check if any tools might be needed based on content
            if content.contains("search") || content.contains("find") {
                yield ToolEvent::ToolExecution {
                    tool_name: "file_search".to_string(),
                    status: "starting".to_string(),
                };

                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                yield ToolEvent::ToolResult {
                    tool_name: "file_search".to_string(),
                    result: serde_json::json!({
                        "found": 0,
                        "query": content
                    }),
                };
            }

            yield ToolEvent::ContentChunk("\n\nI'll help you with that request.".to_string());
            yield ToolEvent::Complete { metadata: None };
        };

        Ok(Box::pin(event_stream))
    }

    pub fn tools_enabled(&self) -> bool {
        CONFIG.enable_chat_tools
    }
}

/// Extension trait for tool executor functionality
#[async_trait]
pub trait ToolExecutorExt {
    async fn execute_tool(&self, tool_name: &str, params: serde_json::Value) -> Result<serde_json::Value>;
    async fn validate_tool_params(&self, tool_name: &str, params: &serde_json::Value) -> Result<bool>;
    async fn handle_tool_call(&self, tool_call: &Value, app_state: &Arc<AppState>) -> Result<Value>;
}

#[async_trait]
impl ToolExecutorExt for ToolExecutor {
    async fn execute_tool(&self, tool_name: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        debug!("Executing tool: {} with params: {:?}", tool_name, params);
        
        // This method is a placeholder - actual tool execution happens in handle_tool_call
        Ok(json!({
            "status": "redirected",
            "message": format!("Tool {} should be executed via handle_tool_call", tool_name)
        }))
    }

    async fn validate_tool_params(&self, tool_name: &str, params: &serde_json::Value) -> Result<bool> {
        debug!("Validating params for tool: {}", tool_name);

        match tool_name {
            "file_search" => Ok(params.get("query").is_some()),
            "code_interpreter" => Ok(params.get("code").is_some()),
            "load_file_context" => Ok(params.get("project_id").is_some()),
            "web_search" => Ok(params.get("query").is_some()),
            "image_generation" => Ok(params.get("prompt").is_some()),
            _ => Ok(false),
        }
    }

    async fn handle_tool_call(&self, tool_call: &Value, app_state: &Arc<AppState>) -> Result<Value> {
        let tool_type = tool_call["type"].as_str()
            .or_else(|| tool_call["function"]["name"].as_str())
            .ok_or_else(|| anyhow!("Missing tool type"))?;
        
        let args = tool_call.get("arguments")
            .or_else(|| tool_call.get("function").and_then(|f| f.get("arguments")))
            .ok_or_else(|| anyhow!("Missing tool arguments"))?;
        
        let parsed_args: Value = if args.is_string() {
            serde_json::from_str(args.as_str().unwrap())?
        } else {
            args.clone()
        };
        
        // Apply timeout based on tool type
        let timeout_duration = match tool_type {
            "code_interpreter" => Duration::from_secs(CONFIG.code_interpreter_timeout),
            "web_search" => Duration::from_secs(CONFIG.web_search_timeout),
            "file_search" => Duration::from_secs(CONFIG.tool_timeout_seconds),
            _ => Duration::from_secs(CONFIG.tool_timeout_seconds),
        };

        with_timeout(
            timeout_duration,
            self.handle_tool_call_internal(tool_type, parsed_args, app_state),
            &format!("Tool call: {}", tool_type),
        ).await
    }
}

impl ToolExecutor {
    async fn handle_tool_call_internal(
        &self,
        tool_type: &str,
        parsed_args: Value,
        app_state: &Arc<AppState>,
    ) -> Result<Value> {
        match tool_type {
            "file_search" => {
                let query = parsed_args["query"].as_str()
                    .ok_or_else(|| anyhow!("Missing query for file search"))?;
                let project_id = parsed_args["project_id"].as_str();
                
                execute_file_search(query, project_id, app_state).await
            }
            
            "load_file_context" => {
                let project_id = parsed_args["project_id"].as_str()
                    .ok_or_else(|| anyhow!("Missing project_id for load_file_context"))?;
                let file_paths = parsed_args["file_paths"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    });
                
                execute_load_file_context(project_id, file_paths, app_state).await
            }
            
            "web_search" => {
                // GPT-5 has native web search capability
                Ok(json!({
                    "tool_type": "web_search",
                    "status": "delegated_to_gpt5",
                    "message": "Web search handled by GPT-5's native capability"
                }))
            }
            
            "code_interpreter" => {
                Ok(json!({
                    "tool_type": "code_interpreter",
                    "status": "not_implemented",
                    "output": "Code execution not yet implemented"
                }))
            }
            
            "image_generation" => {
                let prompt = parsed_args["prompt"].as_str()
                    .ok_or_else(|| anyhow!("Missing prompt for image generation"))?;
                
                // Clone parsed_args before moving it
                execute_image_generation(prompt, parsed_args.clone(), app_state).await
            }
            
            _ => {
                warn!("Unknown tool type: {}", tool_type);
                Err(anyhow!("Unknown tool type: {}", tool_type))
            }
        }
    }
}

// Helper functions for tool execution
async fn execute_file_search(
    query: &str,
    project_id: Option<&str>,
    app_state: &Arc<AppState>,
) -> Result<Value> {
    debug!("Executing file search: query='{}', project_id={:?}", query, project_id);
    
    let params = crate::tools::file_search::FileSearchParams {
        query: query.to_string(),
        file_extensions: None,
        max_files: Some(CONFIG.file_search_max_files),
        case_sensitive: Some(false),
        include_content: Some(true),
    };
    
    let result = app_state.file_search_service
        .search_files(&params, project_id)
        .await?;
    
    Ok(result)
}

async fn execute_load_file_context(
    project_id: &str,
    file_paths: Option<Vec<String>>,
    app_state: &Arc<AppState>,
) -> Result<Value> {
    debug!("Loading file context for project: {}", project_id);
    
    let project = app_state.project_store
        .get_project(project_id)
        .await?
        .ok_or_else(|| anyhow!("Project not found: {}", project_id))?;
    
    let mut files_content = Vec::new();
    let mut total_size = 0usize;
    
    if let Some(paths) = file_paths {
        for path in paths {
            debug!("Would load file: {}", path);
        }
    } else {
        let artifacts = app_state.project_store
            .list_project_artifacts(project_id)
            .await?;
        
        for artifact in artifacts.iter().take(CONFIG.file_search_max_files) {
            if let Some(content) = &artifact.content {
                total_size += content.len();
                files_content.push(json!({
                    "name": artifact.name,
                    "type": format!("{}", artifact.artifact_type),
                    "content": content,
                    "project": project.name.clone()
                }));
            }
        }
    }
    
    Ok(json!({
        "tool_type": "load_file_context",
        "project_id": project_id,
        "files": files_content,
        "file_count": files_content.len(),
        "total_size": total_size,
        "status": "success"
    }))
}

async fn execute_image_generation(
    prompt: &str,
    parsed_args: Value,
    app_state: &Arc<AppState>,
) -> Result<Value> {
    debug!("Executing image generation with prompt: {}", prompt);
    
    // Build image options from parsed arguments
    let options = ImageOptions {
        n: parsed_args["n"].as_u64().map(|n| n as u8),
        size: parsed_args["size"].as_str().map(String::from)
            .or_else(|| Some(CONFIG.image_generation_size.clone())),
        quality: parsed_args["quality"].as_str().map(String::from)
            .or_else(|| Some(CONFIG.image_generation_quality.clone())),
        style: parsed_args["style"].as_str().map(String::from)
            .or_else(|| Some(CONFIG.image_generation_style.clone())),
    };
    
    // Validate options
    options.validate()?;
    
    // Generate images using the image generation manager
    let response: ImageGenerationResponse = app_state.image_generation_manager
        .generate_images(prompt, options)
        .await?;
    
    Ok(json!({
        "tool_type": "image_generation",
        "status": "success",
        "urls": response.urls(),
        "revised_prompt": response.images.first()
            .and_then(|img| img.revised_prompt.as_ref()),
        "count": response.images.len()
    }))
}
