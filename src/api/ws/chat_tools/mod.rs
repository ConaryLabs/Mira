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
use crate::tools::prompt_builder::ToolPromptBuilder;
use crate::llm::responses::{ResponsesManager, types::Message};
use crate::memory::recall::RecallContext;

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
    
    // Get memory context (empty for now, can be enhanced later)
    let context = RecallContext {
        recent: vec![],
        semantic: vec![],
    };
    
    // Get tool definitions as Tool structs
    let tool_structs = crate::tools::definitions::get_enabled_tools();
    
    // Convert Tool structs to Values for passing to ResponsesManager
    let tool_values: Vec<serde_json::Value> = tool_structs.iter()
        .map(|t| serde_json::to_value(t).unwrap_or(json!({})))
        .collect();
    
    // Build system prompt with tool awareness using ToolPromptBuilder
    let system_prompt = ToolPromptBuilder::build_tool_aware_system_prompt(
        &context,
        &tool_structs,
        metadata.as_ref(),
        project_id.as_deref(),
    );
    
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
    
    // Create ResponsesManager and stream response with tools
    let response_manager = ResponsesManager::new(app_state.llm_client.clone());
    
    // Build parameters including tools
    let parameters = json!({
        "verbosity": crate::config::CONFIG.verbosity,
        "reasoning_effort": crate::config::CONFIG.reasoning_effort,
        "max_output_tokens": crate::config::CONFIG.max_output_tokens,
        "tools": tool_values,
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
    
    while let Some(chunk_result) = futures_util::StreamExt::next(&mut stream).await {
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
        
        if let Some(attachment_id) = &meta.attachment_id {
            context_parts.push(format!("Attachment ID: {}", attachment_id));
        }
        
        if !context_parts.is_empty() {
            Some(context_parts.join("\n"))
        } else {
            None
        }
    })
}

/// Save tool interaction to memory
async fn save_tool_interaction(
    app_state: &Arc<AppState>,
    session_id: &str,
    user_message: &str,
    assistant_message: &str,
    project_id: Option<String>,
) -> Result<()> {
    // Save user message
    app_state.memory_service.save_user_message(
        session_id,
        user_message,
        project_id.as_deref(),
    ).await?;
    
    // Create a minimal ChatResponse for the assistant message
    let response = crate::llm::chat_service::ChatResponse {
        output: assistant_message.to_string(),
        persona: "mira".to_string(),
        mood: "helpful".to_string(),
        salience: 5,
        summary: "Tool-assisted response".to_string(),
        memory_type: "Response".to_string(),
        tags: vec!["tool".to_string()],
        intent: None,
        monologue: None,
        reasoning_summary: None,
    };
    
    // Save assistant response
    app_state.memory_service.save_assistant_response(
        session_id,
        &response,
    ).await?;
    
    Ok(())
}
