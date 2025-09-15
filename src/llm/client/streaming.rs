// src/llm/client/streaming.rs
// Consolidated streaming implementation for GPT-5 Responses API

use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::time::{timeout, Duration};
use tracing::{debug, info, warn, error};
use reqwest::{header, Client};

use crate::llm::client::config::ClientConfig;
use crate::state::AppState;

/// Stream of JSON payloads from the OpenAI Responses SSE
pub type ResponseStream = Pin<Box<dyn Stream<Item = Result<Value>> + Send>>;

/// Unified chat event types for streaming responses
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ChatEvent {
    Content { text: String },
    ToolExecution { 
        tool_name: String, 
        status: String 
    },
    ToolResult {
        tool_name: String,
        result: Value,
    },
    Complete {
        mood: Option<String>,
        salience: Option<f32>,
        tags: Option<Vec<String>>,
    },
    Done,
    Error { message: String },
}

/// Create SSE stream for streaming responses
pub async fn create_sse_stream(
    client: &Client,
    config: &ClientConfig,
    body: Value,
) -> Result<ResponseStream> {
    let req = client
        .post(format!("{}/v1/responses", config.base_url()))
        .header(header::AUTHORIZATION, format!("Bearer {}", config.api_key()))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .json(&body);

    let resp = req.send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let error_text = resp.text().await.unwrap_or_else(|_| "<no body>".into());
        return Err(anyhow::anyhow!("OpenAI API error ({}): {}", status, error_text));
    }

    let bytes_stream = resp.bytes_stream();
    let stream = sse_json_stream(bytes_stream);
    Ok(Box::pin(stream))
}

/// Parse SSE stream of JSON into a Stream of Value
pub fn sse_json_stream(
    bytes_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<Value>> + Send {
    bytes_stream
        .map(|chunk_result| {
            chunk_result
                .map_err(|e| anyhow::anyhow!("Stream error: {}", e))
                .and_then(|chunk| {
                    let text = String::from_utf8_lossy(&chunk);
                    parse_sse_chunk(&text)
                })
        })
        .filter_map(|result| async move {
            match result {
                Ok(Some(value)) => Some(Ok(value)),
                Ok(None) => None,  // Skip empty chunks
                Err(e) => Some(Err(e)),
            }
        })
}

/// Parse a single SSE chunk into JSON Value
pub fn parse_sse_chunk(chunk_text: &str) -> Result<Option<Value>> {
    for line in chunk_text.lines() {
        let line = line.trim();
        
        // Skip empty lines and comments
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        
        // Handle SSE event types
        if line.starts_with("event:") {
            let event_type = line.strip_prefix("event:").unwrap_or("").trim();
            debug!("SSE event type: {}", event_type);
            continue;
        }
        
        // Handle data lines
        if line.starts_with("data:") {
            let data_part = line.strip_prefix("data:").unwrap_or("").trim();
            
            // Check for stream end marker
            if data_part == "[DONE]" {
                debug!("Stream completed: [DONE] marker received");
                return Ok(None);
            }
            
            // Skip empty data
            if data_part.is_empty() {
                continue;
            }
            
            // Try to parse as JSON
            match serde_json::from_str::<Value>(data_part) {
                Ok(json_value) => {
                    return Ok(Some(json_value));
                }
                Err(e) => {
                    // Only warn for actual JSON-like content that failed to parse
                    if data_part.starts_with('{') || data_part.starts_with('[') {
                        // Safe preview that respects UTF-8 boundaries
                        let preview = if data_part.len() > 100 {
                            let mut end = 100;
                            while !data_part.is_char_boundary(end) && end > 0 {
                                end -= 1;
                            }
                            format!("{}...", &data_part[..end])
                        } else {
                            data_part.to_string()
                        };
                        warn!("Failed to parse SSE JSON: {} - Data: {}", e, preview);
                    }
                    continue;
                }
            }
        }
    }
    
    // No valid JSON found in this chunk
    Ok(None)
}

/// Process GPT-5 response stream into ChatEvents
/// This is the main streaming logic moved from unified_handler
pub fn process_gpt5_stream(
    mut stream: impl Stream<Item = Result<Value>> + Send + Unpin + 'static,
    has_tools: bool,
    session_id: String,
    app_state: Arc<AppState>,
    project_id: Option<String>,
) -> impl Stream<Item = Result<ChatEvent>> + Send {
    let buffer = Arc::new(std::sync::Mutex::new(String::new()));
    let tool_calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let completion_sent = Arc::new(std::sync::Mutex::new(false));
    let chunk_count = Arc::new(std::sync::Mutex::new(0));
    
    // Create a channel for sending events
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    
    // Spawn a task to process the stream
    tokio::spawn(async move {
        loop {
            match timeout(Duration::from_secs(300), stream.next()).await {
                Ok(Some(chunk_result)) => {
                    match chunk_result {
                        Ok(chunk) => {
                            // Get count and immediately drop the lock
                            let count = {
                                let mut guard = chunk_count.lock().unwrap();
                                *guard += 1;
                                *guard
                            };
                            
                            // Log first 5 chunks in detail for debugging
                            if count <= 5 {
                                info!("RAW CHUNK #{}: {}", count, serde_json::to_string(&chunk).unwrap_or_default());
                            }
                            
                            if let Some(event_type) = chunk.get("type").and_then(|t| t.as_str()) {
                                debug!("Processing event #{} type: {}", count, event_type);
                                
                                match event_type {
                                    // GPT-5 text streaming event - this is what we care about!
                                    "response.output_text.delta" => {
                                        if let Some(delta) = chunk.get("delta").and_then(|d| d.as_str()) {
                                            info!("Got text delta: {} chars", delta.len());
                                            // Update buffer
                                            {
                                                let mut buf = buffer.lock().unwrap();
                                                buf.push_str(delta);
                                            }
                                            let _ = tx.send(Ok(ChatEvent::Content { text: delta.to_string() }));
                                        }
                                    }
                                    
                                    // GPT-5 text completion - marks end of text streaming
                                    "response.output_text.done" => {
                                        // Stream is complete - get buffer content
                                        let final_text = {
                                            let buf = buffer.lock().unwrap();
                                            buf.clone()
                                        };
                                        
                                        info!("Text streaming complete - Final buffer: {} chars", final_text.len());
                                        
                                        if !final_text.is_empty() {
                                            if let Err(e) = save_assistant_to_memory(
                                                &app_state,
                                                &session_id,
                                                &final_text,
                                                project_id.as_deref(),
                                            ).await {
                                                warn!("Failed to save assistant response: {}", e);
                                            }
                                        } else {
                                            warn!("Stream completed but buffer is empty!");
                                        }
                                        
                                        // Set completion flag
                                        {
                                            let mut sent = completion_sent.lock().unwrap();
                                            *sent = true;
                                        }
                                        
                                        let _ = tx.send(Ok(ChatEvent::Done));
                                        break; // Exit the loop after completion
                                    }
                                    
                                    // These are informational events we can safely ignore
                                    "response.created" | "response.in_progress" | 
                                    "response.output_item.added" | "response.output_item.done" => {
                                        debug!("Ignoring informational event: {}", event_type);
                                    }
                                    
                                    // Tool events (if they come through)
                                    "tool_call" if has_tools => {
                                        {
                                            let mut calls = tool_calls.lock().unwrap();
                                            calls.push(chunk.clone());
                                        }
                                        
                                        let tool_name = chunk.get("name")
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("unknown");
                                        
                                        let _ = tx.send(Ok(ChatEvent::ToolExecution {
                                            tool_name: tool_name.to_string(),
                                            status: "started".to_string(),
                                        }));
                                    }
                                    
                                    // Error events
                                    "error" => {
                                        let error_msg = chunk.get("error")
                                            .and_then(|e| e.get("message"))
                                            .and_then(|m| m.as_str())
                                            .unwrap_or("Unknown error");
                                        error!("Stream error: {}", error_msg);
                                        let _ = tx.send(Ok(ChatEvent::Error { message: error_msg.to_string() }));
                                        break;
                                    }
                                    
                                    // Rate limit or other metadata
                                    "rate_limit" | "ping" => {
                                        debug!("Metadata event: {}", event_type);
                                    }
                                    
                                    // Legacy format fallback (shouldn't happen with GPT-5)
                                    "text_delta" => {
                                        warn!("Got legacy text_delta event - API mismatch?");
                                        if let Some(delta) = chunk.get("delta").and_then(|d| d.as_str()) {
                                            {
                                                let mut buf = buffer.lock().unwrap();
                                                buf.push_str(delta);
                                            }
                                            let _ = tx.send(Ok(ChatEvent::Content { text: delta.to_string() }));
                                        }
                                    }
                                    
                                    _ => {
                                        // Only warn about truly unexpected events
                                        if !event_type.starts_with("response.") {
                                            warn!("Unhandled event type: {}", event_type);
                                        }
                                    }
                                }
                            } else {
                                // No type field - shouldn't happen with GPT-5
                                debug!("Chunk #{} without 'type' field", count);
                            }
                        }
                        Err(e) => {
                            error!("Stream error: {}", e);
                            let _ = tx.send(Ok(ChatEvent::Error { 
                                message: format!("Stream error: {}", e) 
                            }));
                            break;
                        }
                    }
                }
                Ok(None) => {
                    // Stream ended naturally
                    info!("Stream ended naturally");
                    break;
                }
                Err(_) => {
                    // Timeout
                    warn!("Stream timeout after 300 seconds - forcing completion");
                    let _ = tx.send(Ok(ChatEvent::Error { 
                        message: "Stream timeout - response may be incomplete".to_string() 
                    }));
                    let _ = tx.send(Ok(ChatEvent::Done));
                    break;
                }
            }
        }
        
        // Check if we sent completion
        let completed = {
            let sent = completion_sent.lock().unwrap();
            *sent
        };
        
        if !completed {
            info!("Sending final Done event");
            let _ = tx.send(Ok(ChatEvent::Done));
        }
    });
    
    // Convert the receiver into a Stream
    tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
}

/// Save assistant response to memory
async fn save_assistant_to_memory(
    app_state: &Arc<AppState>,
    session_id: &str,
    content: &str,
    project_id: Option<&str>,
) -> Result<()> {
    // Create safe UTF-8 summary that won't panic
    let summary = if content.len() > 100 {
        // Find the nearest valid UTF-8 boundary before position 100
        let mut end = 100;
        while !content.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &content[..end])
    } else {
        content.to_string()
    };
    
    let response = crate::llm::types::ChatResponse {
        output: content.to_string(),
        persona: "mira".to_string(),
        mood: "helpful".to_string(),
        salience: 5,
        summary,
        memory_type: "Response".to_string(),
        tags: vec!["chat".to_string()],
        intent: None,
        monologue: None,
        reasoning_summary: None,
    };
    
    app_state.memory_service.save_assistant_response(session_id, &response).await?;
    
    if let Some(proj_id) = project_id {
        debug!("Assistant response saved with project context: {}", proj_id);
    }
    
    Ok(())
}

/// Extract content from streaming chunk (legacy helper)
pub fn extract_content_from_chunk(chunk: &Value) -> Option<String> {
    // Try different paths for content extraction
    
    // 1) GPT-5 Responses API format: delta field
    if let Some(delta) = chunk.get("delta").and_then(|d| d.as_str()) {
        if !delta.is_empty() {
            return Some(delta.to_string());
        }
    }
    
    // 2) Standard delta format: choices[0].delta.content
    if let Some(content) = chunk.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            debug!("Extracted delta content: {} chars", content.len());
            return Some(content.to_string());
        }
    }
    
    // 3) Response API format: output[0].content[0].text
    if let Some(content) = chunk.pointer("/output/0/content/0/text").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            debug!("Extracted response API content: {} chars", content.len());
            return Some(content.to_string());
        }
    }
    
    // 4) Direct content field
    if let Some(content) = chunk.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            debug!("Extracted direct content: {} chars", content.len());
            return Some(content.to_string());
        }
    }
    
    // 5) Text field
    if let Some(content) = chunk.get("text").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            debug!("Extracted text content: {} chars", content.len());
            return Some(content.to_string());
        }
    }
    
    None
}

/// Check if streaming chunk indicates completion
pub fn is_completion_chunk(chunk: &Value) -> bool {
    // Check for GPT-5 response completion events
    if let Some(event_type) = chunk.get("type").and_then(|t| t.as_str()) {
        return event_type == "response.output_text.done" || 
               event_type == "response.done" || 
               event_type == "message_stop";
    }
    
    // Check for finish reasons
    if let Some(finish_reason) = chunk.pointer("/choices/0/finish_reason") {
        return !finish_reason.is_null();
    }
    
    // Check for done field
    chunk.get("done").is_some()
}
