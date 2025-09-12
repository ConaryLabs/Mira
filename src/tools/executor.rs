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
                
                // Use the image generation service with proper ImageOptions struct
                let image_gen_manager = crate::llm::responses::ImageGenerationManager::new(
                    app_state.llm_client.clone()
                );
                
                let options = crate::llm::responses::ImageOptions {
                    n: parsed_args["n"].as_u64().map(|n| n as u8),
                    size: parsed_args["size"].as_str().map(String::from),
                    quality: parsed_args["quality"].as_str().map(String::from),
                    style: parsed_args["style"].as_str().map(String::from),
                };
                
                match image_gen_manager.generate_images(prompt, options).await {
                    Ok(response) => {
                        Ok(json!({
                            "tool_type": "image_generation",
                            "status": "success",
                            "urls": response.urls(),
                            "prompt": prompt
                        }))
                    }
                    Err(e) => {
                        Ok(json!({
                            "tool_type": "image_generation",
                            "status": "error",
                            "error": format!("Failed to generate image: {}", e)
                        }))
                    }
                }
            }
            
            _ => {
                warn!("Unknown tool type: {}", tool_type);
                Ok(json!({
                    "status": "error",
                    "message": format!("Unknown tool type: {}", tool_type)
                }))
            }
        }
    }
}

/// Execute file search using the actual FileSearchService
async fn execute_file_search(
    query: &str,
    project_id: Option<&str>,
    app_state: &Arc<AppState>,
) -> Result<Value> {
    info!("Executing file search for query: '{}'", query);
    
    let params = crate::tools::file_search::FileSearchParams {
        query: query.to_string(),
        file_extensions: None,
        max_files: Some(CONFIG.file_search_max_files),
        case_sensitive: Some(false),
        include_content: Some(true),
    };
    
    app_state.file_search_service
        .search_files(&params, project_id)
        .await
}

/// Load file context from repository
async fn execute_load_file_context(
    project_id: &str,
    file_paths: Option<Vec<String>>,
    app_state: &Arc<AppState>,
) -> Result<Value> {
    info!("Loading file context for project: {}", project_id);
    
    use std::path::Path;
    use tokio::fs;
    
    // Get project from project store
    let project = app_state.project_store
        .get_project(project_id).await?
        .ok_or_else(|| anyhow!("Project not found: {}", project_id))?;
    
    let mut files_content = Vec::new();
    let mut total_size = 0usize;
    const MAX_TOTAL_SIZE: usize = 1_000_000; // 1MB limit
    
    // Use the project directory
    let repo_path = Path::new(&CONFIG.git_repos_dir).join(&project.id);
    
    if !repo_path.exists() {
        return Ok(json!({
            "tool_type": "load_file_context",
            "project_id": project_id,
            "status": "error",
            "message": "Project repository not found"
        }));
    }
    
    // If specific files requested, load those
    if let Some(paths) = file_paths {
        for file_path in paths {
            let full_path = repo_path.join(&file_path);
            if full_path.exists() && !is_binary_file(&full_path) {
                if let Ok(content) = fs::read_to_string(&full_path).await {
                    if total_size + content.len() > MAX_TOTAL_SIZE {
                        warn!("Reached size limit loading files");
                        break;
                    }
                    total_size += content.len();
                    files_content.push(json!({
                        "path": file_path,
                        "content": content,
                        "size": content.len()
                    }));
                }
            }
        }
    } else {
        // Load all non-binary files up to size limit
        if let Ok(mut entries) = fs::read_dir(&repo_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() && !is_binary_file(&path) {
                    if let Ok(content) = fs::read_to_string(&path).await {
                        if total_size + content.len() > MAX_TOTAL_SIZE {
                            warn!("Reached size limit loading files");
                            break;
                        }
                        total_size += content.len();
                        let entry_name = path.strip_prefix(&repo_path)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string();
                        files_content.push(json!({
                            "path": entry_name,
                            "content": content,
                            "project": project.name.clone()
                        }));
                        
                        // Limit to first 20 files
                        if files_content.len() >= 20 {
                            break;
                        }
                    }
                }
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

/// Check if a file is likely binary
fn is_binary_file(path: &std::path::Path) -> bool {
    let binary_extensions = vec![
        "exe", "dll", "so", "dylib", "jar", "class",
        "png", "jpg", "jpeg", "gif", "bmp", "ico", "svg",
        "mp3", "mp4", "avi", "mov", "wmv",
        "zip", "tar", "gz", "rar", "7z",
        "pdf", "doc", "docx", "xls", "xlsx",
        "pyc", "pyo", "o", "a", "lib"
    ];
    
    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        return binary_extensions.contains(&ext_str.as_str());
    }
    
    false
}
