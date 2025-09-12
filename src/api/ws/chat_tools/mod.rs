// src/api/ws/chat_tools/mod.rs
// WebSocket handler for tool-enabled chat messages

use std::sync::Arc;
use anyhow::Result;
use serde_json::{json, Value};
use tracing::{info, error, debug, warn};
use futures_util::SinkExt;

use crate::api::ws::message::{WsServerMessage, MessageMetadata};
use crate::state::AppState;
use crate::tools::executor::ToolExecutor;
use crate::tools::ToolExecutorExt;
use crate::llm::responses::{ResponsesManager, types::Message};

/// Handle a chat message with tool support
pub async fn handle_chat_message_with_tools(
    content: String,
    project_id: Option<String>,
    metadata: Option<MessageMetadata>,
    app_state: Arc<AppState>,
    sender: Arc<tokio::sync::Mutex<futures_util::stream::SplitSink<axum::extract::ws::WebSocket, axum::extract::ws::Message>>>,
    session_id: String,
) -> Result<()> {
    info!("Processing tool-enabled chat for session: {}", session_id);
    
    // Initialize tool executor
    let tool_executor = ToolExecutor::new();
    
    // Build context from metadata
    let file_context = extract_file_context(&metadata);
    
    // Build system prompt with tool awareness
    let system_prompt = build_system_prompt_with_tools(&file_context);
    
    // Create messages in the format ResponsesManager expects
    let input = vec![
        Message {
            role: "system".to_string(),
            content: Some(system_prompt),
            ..Default::default()
        },
        Message {
            role: "user".to_string(),
            content: Some(content.clone()),
            ..Default::default()
        }
    ];
    
    // Get tool definitions
    let tools = get_enabled_tools();
    
    // Create ResponsesManager and stream response with tools
    let response_manager = ResponsesManager::new(app_state.llm_client.clone());
    
    // Build parameters including tools
    let parameters = json!({
        "verbosity": crate::config::CONFIG.verbosity,
        "reasoning_effort": crate::config::CONFIG.reasoning_effort,
        "max_output_tokens": crate::config::CONFIG.max_output_tokens,
        "tools": tools,
    });
    
    // Create streaming response
    let stream = response_manager.create_streaming_response(
        &crate::config::CONFIG.gpt5_model,
        input,
        Some("Respond helpfully using available tools when appropriate.".to_string()),
        Some(&session_id),
        Some(parameters),
    ).await?;
    
    // Process stream
    tokio::pin!(stream);
    
    let mut full_text = String::new();
    let mut tool_calls = Vec::new();
    
    while let Some(chunk_result) = futures::StreamExt::next(&mut stream).await {
        match chunk_result {
            Ok(chunk) => {
                // Extract text content from chunk
                if let Some(text) = chunk.get("content").and_then(|c| c.as_str()) {
                    full_text.push_str(text);
                    
                    // Send directly through WebSocket
                    let msg = WsServerMessage::StreamChunk { text: text.to_string() };
                    let ws_msg = axum::extract::ws::Message::Text(serde_json::to_string(&msg)?);
                    sender.lock().await.send(ws_msg).await?;
                } else if let Some(text) = chunk.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
                    full_text.push_str(text);
                    
                    // Send directly through WebSocket
                    let msg = WsServerMessage::StreamChunk { text: text.to_string() };
                    let ws_msg = axum::extract::ws::Message::Text(serde_json::to_string(&msg)?);
                    sender.lock().await.send(ws_msg).await?;
                }
                
                // Check for tool calls in the chunk
                if let Some(tc) = chunk.get("tool_call") {
                    debug!("Received tool call: {:?}", tc);
                    tool_calls.push(tc.clone());
                }
                
                // Check for completion
                if chunk.get("done").is_some() || 
                   chunk.pointer("/choices/0/finish_reason").is_some() {
                    debug!("Stream complete");
                    break;
                }
            }
            Err(e) => {
                error!("Stream processing error: {}", e);
                
                // Send error through WebSocket
                let msg = WsServerMessage::Error {
                    message: format!("Stream error: {}", e),
                    code: "STREAM_ERROR".to_string(),
                };
                let ws_msg = axum::extract::ws::Message::Text(serde_json::to_string(&msg)?);
                sender.lock().await.send(ws_msg).await?;
                break;
            }
        }
    }
    
    // Execute any tool calls
    if !tool_calls.is_empty() {
        info!("Executing {} tool calls", tool_calls.len());
        
        for tool_call in tool_calls {
            let result = tool_executor.handle_tool_call(&tool_call, &app_state).await;
            
            let tool_message = match result {
                Ok(output) => {
                    format!("Tool {} completed:\n{}", 
                        tool_call.get("type").and_then(|t| t.as_str()).unwrap_or("unknown"),
                        serde_json::to_string_pretty(&output).unwrap_or_default()
                    )
                }
                Err(e) => {
                    format!("Tool {} failed: {}", 
                        tool_call.get("type").and_then(|t| t.as_str()).unwrap_or("unknown"),
                        e
                    )
                }
            };
            
            // Send tool result through WebSocket
            let msg = WsServerMessage::StreamChunk { 
                text: format!("\n\n{}\n", tool_message) 
            };
            let ws_msg = axum::extract::ws::Message::Text(serde_json::to_string(&msg)?);
            sender.lock().await.send(ws_msg).await?;
        }
    }
    
    // Save to memory
    if let Err(e) = save_tool_interaction(&app_state, &session_id, &content, &full_text, project_id).await {
        warn!("Failed to save tool interaction to memory: {}", e);
    }
    
    // Send completion signals through WebSocket
    let end_msg = WsServerMessage::StreamEnd;
    let ws_end = axum::extract::ws::Message::Text(serde_json::to_string(&end_msg)?);
    sender.lock().await.send(ws_end).await?;
    
    let done_msg = WsServerMessage::Done;
    let ws_done = axum::extract::ws::Message::Text(serde_json::to_string(&done_msg)?);
    sender.lock().await.send(ws_done).await?;
    
    Ok(())
}

/// Get enabled tool definitions
fn get_enabled_tools() -> Vec<Value> {
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
    
    // Image generation with gpt-image-1
    if crate::config::CONFIG.enable_image_generation {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "image_generation",
                "description": "Generate an image from a text description using gpt-image-1",
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
                            "enum": ["1024x1024", "1792x1024", "1024x1792"]
                        },
                        "quality": {
                            "type": "string",
                            "description": "Image quality",
                            "enum": ["standard", "hd"]
                        },
                        "style": {
                            "type": "string", 
                            "description": "Image style",
                            "enum": ["vivid", "natural"]
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
        .save_user_message(session_id, user_message, project_id.as_deref())
        .await?;
    
    // Create response object for memory with actual ChatResponse fields
    let response = crate::llm::chat_service::ChatResponse {
        output: assistant_response.to_string(),
        salience: 5,
        summary: format!("Tool-assisted response about: {}", 
            user_message.chars().take(50).collect::<String>()),
        reasoning_summary: None,
        mood: String::new(),
        tags: vec!["tools".to_string()],
        intent: None,
        memory_type: String::from("interaction"),
        monologue: None,
        persona: String::from("Mira"),
    };
    
    // Save assistant response
    let _assistant_id = app_state.memory_service
        .save_assistant_response(session_id, &response)
        .await?;
    
    Ok(())
}
