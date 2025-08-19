// src/api/ws/chat_tools.rs
// Phase 3: WebSocket handler updates for GPT-5 tool support

use std::sync::Arc;
use axum::extract::ws::Message;
use futures_util::stream::SplitSink;
use futures::SinkExt;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::info;
use anyhow::Result;

use crate::api::ws::message::{WsClientMessage, MessageMetadata};
use crate::state::AppState;
use crate::services::chat_with_tools::ChatServiceToolExt;

/// Enhanced WebSocket server messages with tool support
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum WsServerMessageWithTools {
    // Existing message types
    Chunk { 
        content: String, 
        mood: Option<String> 
    },
    Complete { 
        mood: Option<String>, 
        salience: Option<f32>, 
        tags: Option<Vec<String>> 
    },
    Status { 
        message: String, 
        detail: Option<String> 
    },
    Aside { 
        emotional_cue: String, 
        intensity: Option<f32> 
    },
    Error { 
        message: String,
        code: Option<String>
    },
    Done,
    
    // New tool-related message types
    ToolResult { 
        tool_type: String, 
        data: Value 
    },
    Citation { 
        file_id: String, 
        filename: String, 
        url: Option<String>,
        snippet: Option<String>
    },
}

/// Handle chat message with tool support
pub async fn handle_chat_message_with_tools(
    content: String,
    project_id: Option<String>,
    metadata: Option<MessageMetadata>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    info!("🚀 Processing chat message with tools for session: {}", session_id);
    
    // 1. Send initial status
    let status_msg = WsServerMessageWithTools::Status {
        message: "Processing your request...".to_string(),
        detail: Some("Checking available tools".to_string()),
    };
    sender.lock().await.send(Message::Text(
        serde_json::to_string(&status_msg)?
    )).await?;
    
    // 2. Extract file context from metadata
    let file_context = metadata.as_ref().map(|m| {
        json!({
            "file_path": m.file_path,
            "repo_id": m.repo_id,
            "attachment_id": m.attachment_id,
            "language": m.language,
            "selection": m.selection,
        })
    });
    
    // 3. Call chat_with_tools using the extension trait
    info!("📡 Calling chat service with tools enabled");
    let response = match app_state.chat_service.chat_with_tools(
        &session_id,
        &content,
        project_id.as_deref(),
        file_context,
    ).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("Chat with tools failed: {}", e);
            let error_msg = WsServerMessageWithTools::Error {
                message: "Failed to process request with tools".to_string(),
                code: Some("TOOL_ERROR".to_string()),
            };
            sender.lock().await.send(Message::Text(
                serde_json::to_string(&error_msg)?
            )).await?;
            return Ok(());
        }
    };
    
    // 4. Send main response as chunks
    info!("📤 Sending main response");
    let chunk_size = 100; // Characters per chunk
    let output_chars: Vec<char> = response.base.output.chars().collect();
    
    for chunk in output_chars.chunks(chunk_size) {
        let chunk_text: String = chunk.iter().collect();
        let chunk_msg = WsServerMessageWithTools::Chunk {
            content: chunk_text,
            mood: Some(response.base.mood.clone()),
        };
        sender.lock().await.send(Message::Text(
            serde_json::to_string(&chunk_msg)?
        )).await?;
        
        // Small delay for streaming effect
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
    }
    
    // 5. Send tool results if any
    if let Some(tool_results) = response.tool_results {
        info!("📊 Sending {} tool results", tool_results.len());
        
        for result in tool_results {
            let tool_type = result["type"].as_str().unwrap_or("unknown");
            
            // Send status for tool execution
            let tool_status = WsServerMessageWithTools::Status {
                message: format!("Executed tool: {}", tool_type),
                detail: None,
            };
            sender.lock().await.send(Message::Text(
                serde_json::to_string(&tool_status)?
            )).await?;
            
            // Send actual tool result
            let tool_msg = WsServerMessageWithTools::ToolResult {
                tool_type: tool_type.to_string(),
                data: result,
            };
            sender.lock().await.send(Message::Text(
                serde_json::to_string(&tool_msg)?
            )).await?;
        }
    }
    
    // 6. Send citations if any
    if let Some(citations) = response.citations {
        info!("📚 Sending {} citations", citations.len());
        
        for citation in citations {
            let citation_msg = WsServerMessageWithTools::Citation {
                file_id: citation["file_id"].as_str().unwrap_or("").to_string(),
                filename: citation["filename"].as_str().unwrap_or("").to_string(),
                url: citation["url"].as_str().map(String::from),
                snippet: citation["snippet"].as_str().map(String::from),
            };
            sender.lock().await.send(Message::Text(
                serde_json::to_string(&citation_msg)?
            )).await?;
        }
    }
    
    // 7. Send complete message with metadata
    let complete_msg = WsServerMessageWithTools::Complete {
        mood: Some(response.base.mood),
        salience: Some(response.base.salience as f32),
        tags: Some(response.base.tags),
    };
    sender.lock().await.send(Message::Text(
        serde_json::to_string(&complete_msg)?
    )).await?;
    
    // 8. Send done marker
    let done_msg = WsServerMessageWithTools::Done;
    sender.lock().await.send(Message::Text(
        serde_json::to_string(&done_msg)?
    )).await?;
    
    info!("✅ Successfully sent response with tools");
    Ok(())
}

/// Update the main WebSocket handler to use tools
pub async fn update_ws_handler_for_tools(
    msg: WsClientMessage,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    match msg {
        WsClientMessage::Chat { content, project_id, metadata } => {
            // Use the new tool-enabled handler
            handle_chat_message_with_tools(
                content,
                project_id,
                metadata,
                app_state,
                sender,
                session_id,
            ).await
        }
        WsClientMessage::Message { content, project_id, .. } => {
            // Legacy format - convert to chat with no metadata
            handle_chat_message_with_tools(
                content,
                project_id,
                None,
                app_state,
                sender,
                session_id,
            ).await
        }
        _ => {
            // Handle other message types as before
            Ok(())
        }
    }
}
