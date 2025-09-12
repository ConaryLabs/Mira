// src/tools/executor.rs
// Tool execution framework with real implementations

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};
use std::sync::Arc;
use futures::Stream;
use std::pin::Pin;

use crate::state::AppState;
use crate::config::CONFIG;
use crate::llm::responses::ResponsesManager;
use crate::memory::recall::RecallContext;
use crate::api::ws::message::MessageMetadata;

// Define the ToolExecutor struct
pub struct ToolExecutor {
    responses_manager: Option<Arc<ResponsesManager>>,
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self {
            responses_manager: None,
        }
    }
    
    pub fn with_responses_manager(responses_manager: Arc<ResponsesManager>) -> Self {
        Self {
            responses_manager: Some(responses_manager),
        }
    }
    
    pub fn tools_enabled(&self) -> bool {
        CONFIG.enable_chat_tools
    }
    
    pub async fn stream_with_tools(
        &self,
        request: &ToolChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ToolEvent> + Send + 'static>>> {
        // Use the actual streaming implementation from ResponsesManager
        if let Some(ref manager) = self.responses_manager {
            // Build input messages
            let mut input = vec![
                crate::llm::responses::types::Message {
                    role: "system".to_string(),
                    content: Some(request.system_prompt.clone()),
                    ..Default::default()
                }
            ];
            
            input.push(crate::llm::responses::types::Message {
                role: "user".to_string(),
                content: Some(request.content.clone()),
                ..Default::default()
            });
            
            // Get enabled tools
            let tools = if CONFIG.enable_chat_tools {
                Some(crate::tools::definitions::get_enabled_tools())
            } else {
                None
            };
            
            // Create streaming response using ResponsesManager's actual method
            let stream = manager.create_streaming_response(
                &CONFIG.gpt5_model,
                input,
                Some("You are Mira, a helpful AI assistant with access to tools.".to_string()),
                Some(&request.session_id),
                Some(json!({
                    "verbosity": CONFIG.verbosity,
                    "reasoning_effort": CONFIG.reasoning_effort,
                    "max_output_tokens": CONFIG.max_output_tokens,
                })),
            ).await?;
            
            // Convert the stream to ToolEvents
            use futures::StreamExt;
            let event_stream = stream.map(move |chunk_result| {
                match chunk_result {
                    Ok(chunk) => {
                        // Extract text from the chunk
                        if let Some(text) = chunk.get("content").and_then(|c| c.as_str()) {
                            ToolEvent::ContentChunk(text.to_string())
                        } else if let Some(text) = chunk.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
                            ToolEvent::ContentChunk(text.to_string())
                        } else if chunk.get("done").is_some() {
                            ToolEvent::Done
                        } else {
                            ToolEvent::Error("Unknown chunk format".to_string())
                        }
                    }
                    Err(e) => ToolEvent::Error(format!("Stream error: {}", e))
                }
            });
            
            Ok(Box::pin(event_stream))
        } else {
            // Fallback: use the simple streaming from llm::streaming module
            let client = crate::llm::client::OpenAIClient::new()?;
            let stream = crate::llm::streaming::start_response_stream(
                &*client,  // Dereference the Arc
                &request.content,
                Some(&request.system_prompt),
                false,
            ).await?;
            
            use futures::StreamExt;
            let event_stream = stream.map(move |result| {
                match result {
                    Ok(event) => {
                        use crate::llm::streaming::StreamEvent;
                        match event {
                            StreamEvent::Delta(text) | StreamEvent::Text(text) => {
                                ToolEvent::ContentChunk(text)
                            }
                            StreamEvent::Done { .. } => ToolEvent::Done,
                            StreamEvent::Error(e) => ToolEvent::Error(e),
                        }
                    }
                    Err(e) => ToolEvent::Error(format!("Stream error: {}", e))
                }
            });
            
            Ok(Box::pin(event_stream))
        }
    }
}

// Define the request and event types
#[derive(Debug, Clone)]
pub struct ToolChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
    pub context: RecallContext,
    pub system_prompt: String,
}

#[derive(Debug, Clone)]
pub enum ToolEvent {
    ContentChunk(String),
    ToolCallStarted { tool_type: String, tool_id: String },
    ToolCallCompleted { tool_type: String, tool_id: String, result: Value },
    ToolCallFailed { tool_type: String, tool_id: String, error: String },
    ImageGenerated { urls: Vec<String>, revised_prompt: Option<String> },
    Complete { metadata: Option<ResponseMetadata> },
    Error(String),
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub mood: Option<String>,
    pub salience: Option<f32>,  // Changed from u8 to f32 to match WsServerMessage
    pub tags: Option<Vec<String>>,
}

/// Execute file search using the actual FileSearchService
pub async fn execute_file_search(
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

/// Load file context from repository - REAL IMPLEMENTATION
pub async fn execute_load_file_context(
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
    
    // Use the project directory directly since get_project_attachments doesn't exist
    let repo_path = Path::new(&CONFIG.git_repos_dir).join(&project.id);
    
    if !repo_path.exists() {
        return Ok(json!({
            "tool_type": "load_file_context",
            "project_id": project_id,
            "status": "no_repository",
            "message": "Project repository directory does not exist"
        }));
    }
    
    if let Some(ref paths) = file_paths {
        // Load specific files
        for file_path in paths {
            let full_path = repo_path.join(file_path);
            
            if full_path.exists() && full_path.is_file() {
                if let Ok(metadata) = fs::metadata(&full_path).await {
                    let size = metadata.len() as usize;
                    
                    if total_size + size > MAX_TOTAL_SIZE {
                        warn!("Skipping file {} - would exceed size limit", file_path);
                        continue;
                    }
                    
                    if let Ok(content) = fs::read_to_string(&full_path).await {
                        files_content.push(json!({
                            "path": file_path,
                            "content": content,
                            "size": size,
                            "project": project.name.clone()
                        }));
                        total_size += size;
                    }
                }
            }
        }
    } else {
        // Load README or overview
        for entry_name in &["README.md", "readme.md", "README.txt", "readme.txt"] {
            let readme_path = repo_path.join(entry_name);
            if readme_path.exists() {
                if let Ok(content) = fs::read_to_string(&readme_path).await {
                    files_content.push(json!({
                        "path": entry_name,
                        "content": content,
                        "project": project.name.clone()
                    }));
                    break;
                }
            }
        }
    }
    
    Ok(json!({
        "tool_type": "load_file_context",
        "project_id": project_id,
        "files": files_content,
        "file_count": files_content.len(),
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

/// Extension trait to add tool execution methods
pub trait ToolExecutorExt {
    async fn handle_tool_call(&self, tool_call: &Value, app_state: &Arc<AppState>) -> Result<Value>;
}

impl ToolExecutorExt for ToolExecutor {
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
                // Use the REAL file search implementation
                let query = parsed_args["query"].as_str()
                    .ok_or_else(|| anyhow!("Missing query for file search"))?;
                let project_id = parsed_args["project_id"].as_str();
                
                execute_file_search(query, project_id, app_state).await
            }
            
            "web_search" => {
                // Web search through GPT-5's native capability
                let query = parsed_args["query"].as_str()
                    .ok_or_else(|| anyhow!("Missing query for web search"))?;
                
                // GPT-5 has native web search - just indicate it should be used
                Ok(json!({
                    "tool_type": "web_search",
                    "query": query,
                    "status": "delegated_to_gpt5",
                    "message": "Web search is handled natively by GPT-5"
                }))
            }
            
            "code_interpreter" => {
                // Code interpreter through GPT-5's native capability
                let code = parsed_args["code"].as_str()
                    .ok_or_else(|| anyhow!("Missing code for interpreter"))?;
                let language = parsed_args["language"].as_str().unwrap_or("python");
                
                Ok(json!({
                    "tool_type": "code_interpreter",
                    "code": code,
                    "language": language,
                    "status": "delegated_to_gpt5",
                    "message": "Code execution is handled natively by GPT-5"
                }))
            }
            
            "image_generation" => {
                // Use REAL image generation through gpt-image-1
                let prompt = parsed_args["prompt"].as_str()
                    .ok_or_else(|| anyhow!("Missing prompt for image generation"))?;
                
                let manager = crate::llm::responses::ImageGenerationManager::new(
                    app_state.llm_client.clone()
                );
                
                let options = crate::llm::responses::ImageOptions {
                    n: parsed_args["n"].as_u64().map(|n| n as u8).or(Some(1)),
                    size: parsed_args["size"].as_str().map(String::from).or(Some("1024x1024".to_string())),
                    quality: parsed_args["quality"].as_str().map(String::from).or(Some("standard".to_string())),
                    style: parsed_args["style"].as_str().map(String::from).or(Some("vivid".to_string())),
                };
                
                // Validate and generate
                options.validate()?;
                let response = manager.generate_images(prompt, options).await?;
                
                Ok(json!({
                    "tool_type": "image_generation",
                    "status": "success",
                    "images": response.images,
                    "model": response.model,
                }))
            }
            
            "load_file_context" => {
                let project_id = parsed_args["project_id"].as_str()
                    .ok_or_else(|| anyhow!("Missing project_id for file context"))?;
                let file_paths = parsed_args["file_paths"]
                    .as_array()
                    .map(|arr| arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect());
                execute_load_file_context(project_id, file_paths, app_state).await
            }
            
            _ => {
                warn!("Unknown tool type: {}", tool_type);
                Ok(json!({
                    "tool_type": tool_type,
                    "status": "unknown_tool",
                    "error": format!("Tool '{}' is not implemented", tool_type)
                }))
            }
        }
    }
}
