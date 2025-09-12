// src/api/ws/chat_tools/mod.rs
// WebSocket handler for tool-enabled chat messages

use std::sync::Arc;
use anyhow::Result;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{info, error, debug, warn};

use crate::api::ws::message::{WsServerMessage, MessageMetadata};
use crate::state::AppState;
use crate::tools::executor::{ToolRegistry, ToolCall};
use crate::llm::responses::ResponseManager;
use crate::llm::streaming::processor::StreamEvent;

/// Handle a chat message with tool support
pub async fn handle_chat_message_with_tools(
    content: String,
    project_id: Option<String>,
    metadata: Option<MessageMetadata>,
    app_state: Arc<AppState>,
    sender: mpsc::UnboundedSender<WsServerMessage>,
    session_id: String,
) -> Result<()> {
    info!("Processing tool-enabled chat for session: {}", session_id);
    
    // Initialize tool registry
    let tool_registry = Arc::new(ToolRegistry::new(app_state.clone()));
    
    // Build context from metadata
    let file_context = extract_file_context(&metadata);
    
    // Create messages with context
    let mut messages = vec![
        json!({
            "role": "system",
            "content": build_system_prompt_with_tools(&file_context)
        })
    ];
    
    // Add user message
    messages.push(json!({
        "role": "user",
        "content": content
    }));
    
    // Get tool definitions
    let tools = get_enabled_tools(&app_state);
    
    // Stream response with tools
    let response_manager = ResponseManager::new(app_state.llm_client.clone());
    
    let stream = response_manager.stream_with_tools(
        messages,
        tools,
        session_id.clone(),
    ).await?;
    
    // Process stream
    tokio::pin!(stream);
    
    let mut full_text = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    
    while let Some(event) = futures::StreamExt::next(&mut stream).await {
        match event {
            Ok(StreamEvent::Delta(text)) => {
                full_text.push_str(&text);
                sender.send(WsServerMessage::StreamChunk { text })?;
            }
            Ok(StreamEvent::ToolCall(tc)) => {
                debug!("Received tool call: {:?}", tc);
                tool_calls.push(tc);
            }
            Ok(StreamEvent::Done { .. }) => {
                debug!("Stream complete");
                break;
            }
            Ok(StreamEvent::Error(e)) => {
                error!("Stream error: {}", e);
                sender.send(WsServerMessage::Error {
                    message: e,
                    code: "STREAM_ERROR".to_string(),
                })?;
                break;
            }
            Err(e) => {
                error!("Stream processing error: {}", e);
                break;
            }
            _ => {}
        }
    }
    
    // Execute any tool calls
    if !tool_calls.is_empty() {
        info!("Executing {} tool calls", tool_calls.len());
        
        let results = tool_registry.execute_tool_calls(tool_calls).await;
        
        // Send tool results back to the user
        for result in results {
            let tool_message = if let Some(output) = result.result {
                format!("🔧 {} completed:\n{}", 
                    result.tool_name,
                    serde_json::to_string_pretty(&output).unwrap_or_default()
                )
            } else if let Some(error) = result.error {
                format!("❌ {} failed: {}", result.tool_name, error)
            } else {
                format!("🔧 {} completed", result.tool_name)
            };
            
            sender.send(WsServerMessage::StreamChunk { 
                text: format!("\n\n{}\n", tool_message) 
            })?;
        }
    }
    
    // Save to memory
    if let Err(e) = save_tool_interaction(&app_state, &session_id, &content, &full_text, project_id).await {
        warn!("Failed to save tool interaction to memory: {}", e);
    }
    
    // Send completion signal
    sender.send(WsServerMessage::StreamEnd)?;
    sender.send(WsServerMessage::Done)?;
    
    Ok(())
}

/// Build system prompt with tool context
fn build_system_prompt_with_tools(file_context: &Option<String>) -> String {
    let mut prompt = String::from("You are Mira, an AI development assistant with access to various tools. ");
    
    if let Some(context) = file_context {
        prompt.push_str(&format!("\n\nFile context:\n{}", context));
    }
    
    prompt.push_str("\n\nYou have access to the following tools:");
    prompt.push_str("\n- file_search: Search for files and code in the project");
    prompt.push_str("\n- load_file_context: Load full file contents from the repository");
    
    if crate::config::CONFIG.enable_web_search {
        prompt.push_str("\n- web_search: Search the web for current information");
    }
    
    if crate::config::CONFIG.enable_code_interpreter {
        prompt.push_str("\n- code_interpreter: Execute code in a sandboxed environment");
    }
    
    if crate::config::CONFIG.enable_image_generation {
        prompt.push_str("\n- generate_image: Create images from text descriptions");
    }
    
    prompt.push_str("\n\nUse tools when they would help answer the user's question more accurately.");
    
    prompt
}

/// Get enabled tool definitions
fn get_enabled_tools(app_state: &AppState) -> Vec<Value> {
    let mut tools = vec![
        // File search tool
        json!({
            "type": "function",
            "function": {
                "name": "file_search",
                "description": "Search for files and code snippets in the project",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query for files or code"
                        },
                        "project_id": {
                            "type": "string",
                            "description": "Optional project ID to search within"
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum number of results to return (default: 10)"
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
        
        // File context loader
        json!({
            "type": "function",
            "function": {
                "name": "load_file_context",
                "description": "Load full contents of files from the repository",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project_id": {
                            "type": "string",
                            "description": "Project ID to load files from"
                        },
                        "file_paths": {
                            "type": "array",
                            "items": {
                                "type": "string"
                            },
                            "description": "List of file paths to load (optional, loads all if not specified)"
                        }
                    },
                    "required": ["project_id"]
                }
            }
        }),
    ];
    
    // Web search tool
    if crate::config::CONFIG.enable_web_search {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the web for current information",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        }
                    },
                    "required": ["query"]
                }
            }
        }));
    }
    
    // Code interpreter
    if crate::config::CONFIG.enable_code_interpreter {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "code_interpreter",
                "description": "Execute code in a sandboxed environment",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "Code to execute"
                        },
                        "language": {
                            "type": "string",
                            "description": "Programming language (python, javascript, etc.)",
                            "enum": ["python", "javascript", "rust", "go", "java"]
                        }
                    },
                    "required": ["code"]
                }
            }
        }));
    }
    
    // Image generation
    if crate::config::CONFIG.enable_image_generation {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "generate_image",
                "description": "Generate an image from a text description",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "Text description of the image to generate"
                        },
                        "size": {
                            "type": "string",
                            "description": "Image size",
                            "enum": ["256x256", "512x512", "1024x1024", "1792x1024", "1024x1792"]
                        },
                        "quality": {
                            "type": "string",
                            "description": "Image quality",
                            "enum": ["standard", "hd"]
                        },
                        "n": {
                            "type": "integer",
                            "description": "Number of images to generate (1-10)",
                            "minimum": 1,
                            "maximum": 10
                        }
                    },
                    "required": ["prompt"]
                }
            }
        }));
    }
    
    tools
}

/// Extract file context from metadata
fn extract_file_context(metadata: &Option<MessageMetadata>) -> Option<String> {
    metadata.as_ref().and_then(|meta| {
        let mut context_parts = Vec::new();
        
        if let Some(file_path) = &meta.file_path {
            context_parts.push(format!("Current file: {}", file_path));
        }
        
        if let Some(repo_id) = &meta.repo_id {
            context_parts.push(format!("Repository: {}", repo_id));
        }
        
        if let Some(language) = &meta.language {
            context_parts.push(format!("Language: {}", language));
        }
        
        if let Some(selection) = &meta.selection {
            context_parts.push(format!(
                "Selected lines {}-{}", 
                selection.start_line, 
                selection.end_line
            ));
            
            if let Some(text) = &selection.text {
                context_parts.push(format!("Selected text:\n{}", text));
            }
        }
        
        if context_parts.is_empty() {
            None
        } else {
            Some(context_parts.join("\n"))
        }
    })
}

/// Save tool interaction to memory
async fn save_tool_interaction(
    app_state: &AppState,
    session_id: &str,
    user_message: &str,
    assistant_response: &str,
    project_id: Option<String>,
) -> Result<()> {
    // Save user message
    let _user_id = app_state.memory_service
        .save_user_message(session_id, user_message)
        .await?;
    
    // Create response object for memory
    let response = crate::llm::chat_service::ChatResponse {
        output: assistant_response.to_string(),
        salience: 5, // Tool interactions are moderately important
        summary: format!("Tool-assisted response about: {}", 
            user_message.chars().take(50).collect::<String>()
        ),
        mood: None,
        tags: Some(vec!["tools".to_string()]),
    };
    
    // Save assistant response
    let _assistant_id = app_state.memory_service
        .save_assistant_response(session_id, &response)
        .await?;
    
    Ok(())
}
