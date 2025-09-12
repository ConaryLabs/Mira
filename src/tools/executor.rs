// src/tools/executor.rs
// Tool execution framework for coordinating tool-enabled responses.
// Manages the execution of various tools and streaming of results.
// Updated to include tool decision logic

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
        // Check if tools are disabled globally
        if !CONFIG.enable_chat_tools {
            return false;
        }
        
        // Check metadata for context that would benefit from tools
        if let Some(meta) = metadata {
            // If we have file context, repository, or attachments, use tools
            if meta.file_path.is_some() || 
               meta.repo_id.is_some() || 
               meta.attachment_id.is_some() {
                debug!("Using tools due to file/repo/attachment context");
                return true;
            }
            
            // If we have language context, use tools
            if meta.language.is_some() {
                debug!("Using tools due to language context");
                return true;
            }
        }
        
        // Default to not using tools for simple messages
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

        // Check if tools are enabled
        if !CONFIG.enable_chat_tools {
            warn!("Tools disabled in config, returning error event");
            let event_stream = futures_util::stream::once(async move {
                ToolEvent::Error("Tools are not enabled".to_string())
            });
            return Ok(Box::pin(event_stream));
        }

        // Get enabled tools based on configuration
        let _tools = if CONFIG.enable_chat_tools {
            vec![
                "file_search".to_string(),
                "code_interpreter".to_string(),
                "image_generation".to_string(),
                "web_search".to_string(),
            ]
        } else {
            vec![]
        };

        // Simplified streaming implementation
        // In production, this would integrate with the actual tool execution pipeline
        let content = request.content.clone();
        let session_id = request.session_id.clone();

        let event_stream = async_stream::stream! {
            info!("Processing request for session: {}", session_id);

            // Simulate initial response
            yield ToolEvent::ContentChunk(format!("Processing your request: {}", content.chars().take(50).collect::<String>()));

            // Check if any tools might be needed based on content
            if content.contains("search") || content.contains("find") {
                yield ToolEvent::ToolExecution {
                    tool_name: "file_search".to_string(),
                    status: "starting".to_string(),
                };

                // Simulate tool execution delay
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                yield ToolEvent::ToolResult {
                    tool_name: "file_search".to_string(),
                    result: serde_json::json!({
                        "found": 0,
                        "query": content
                    }),
                };
            }

            // Final response
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

        match tool_name {
            "file_search" => {
                // Placeholder for file search implementation
                Ok(serde_json::json!({
                    "status": "completed",
                    "results": []
                }))
            }
            "code_interpreter" => {
                // Placeholder for code interpreter
                Ok(serde_json::json!({
                    "status": "completed",
                    "output": "Code execution not yet implemented"
                }))
            }
            _ => {
                warn!("Unknown tool requested: {}", tool_name);
                Ok(serde_json::json!({
                    "status": "error",
                    "message": format!("Unknown tool: {}", tool_name)
                }))
            }
        }
    }

    async fn validate_tool_params(&self, tool_name: &str, params: &serde_json::Value) -> Result<bool> {
        debug!("Validating params for tool: {}", tool_name);

        match tool_name {
            "file_search" => {
                // Validate file search params
                Ok(params.get("query").is_some())
            }
            "code_interpreter" => {
                // Validate code interpreter params
                Ok(params.get("code").is_some())
            }
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
        
        // Parse arguments if they're a string
        let parsed_args: Value = if args.is_string() {
            serde_json::from_str(args.as_str().unwrap())?
        } else {
            args.clone()
        };
        
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
                let query = parsed_args["query"].as_str()
                    .ok_or_else(|| anyhow!("Missing query for web search"))?;
                
                // GPT-5 has native web search - just indicate it should be used
                Ok(json!({
                    "tool_type": "web_search",
                    "query": query,
                    "status": "delegated_to_gpt5",
                    "message": "Web search handled by GPT-5's native capability"
                }))
            }
            
            "code_interpreter" => {
                let code = parsed_args["code"].as_str()
                    .ok_or_else(|| anyhow!("Missing code for interpreter"))?;
                let language = parsed_args["language"].as_str()
                    .unwrap_or("python");
                
                // Placeholder for code execution
                Ok(json!({
                    "tool_type": "code_interpreter",
                    "language": language,
                    "code": code,
                    "status": "not_implemented",
                    "output": "Code execution not yet implemented"
                }))
            }
            
            "image_generation" => {
                let prompt = parsed_args["prompt"].as_str()
                    .ok_or_else(|| anyhow!("Missing prompt for image generation"))?;
                
                // This would integrate with image_generation_manager in AppState
                Ok(json!({
                    "tool_type": "image_generation",
                    "prompt": prompt,
                    "status": "not_implemented",
                    "message": "Image generation not yet connected"
                }))
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
    
    // Use the file_search_service from AppState
    let params = crate::tools::file_search::FileSearchParams {
        query: query.to_string(),
        file_extensions: None,
        max_files: Some(20),
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
    
    // Get project from store
    let project = app_state.project_store
        .get_project(project_id)
        .await?
        .ok_or_else(|| anyhow!("Project not found: {}", project_id))?;
    
    let mut files_content = Vec::new();
    let mut total_size = 0usize;
    
    // If specific files requested, load those
    if let Some(paths) = file_paths {
        for path in paths {
            // This would load specific files
            debug!("Would load file: {}", path);
        }
    } else {
        // Load all artifacts from the project
        let artifacts = app_state.project_store
            .list_project_artifacts(project_id)
            .await?;
        
        for artifact in artifacts.iter().take(20) {
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
