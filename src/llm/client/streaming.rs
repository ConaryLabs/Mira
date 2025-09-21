// src/llm/client/streaming.rs
// Streaming implementation for GPT-5 Responses API with GIGACHAD batching

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use futures::stream::unfold;
use serde::Serialize;
use serde_json::Value;
use tokio::time::timeout;
use tokio_stream::wrappers::ReceiverStream;
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

/// Parse SSE stream of JSON into a Stream of Value with proper buffering
pub fn sse_json_stream(
    bytes_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<Value>> + Send {
    // Pin the stream so we can call next() on it
    let pinned_stream = Box::pin(bytes_stream);
    // Initial state: (stream, buffer)
    let initial_state = (pinned_stream, String::new());
    
    unfold(initial_state, |(mut stream, mut buffer)| async move {
        loop {
            // First, try to parse what's in the buffer
            if let Some(value) = try_parse_buffer(&mut buffer) {
                return Some((Ok(value), (stream, buffer)));
            }
            
            // Buffer doesn't have a complete message, get more data
            match stream.next().await {
                Some(Ok(bytes)) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);
                    // Loop continues to try parsing again
                }
                Some(Err(e)) => {
                    return Some((Err(anyhow::anyhow!("Stream error: {}", e)), (stream, buffer)));
                }
                None => {
                    // Stream ended - try one more parse of remaining buffer
                    if let Some(value) = try_parse_buffer(&mut buffer) {
                        return Some((Ok(value), (stream, buffer)));
                    }
                    
                    if !buffer.trim().is_empty() {
                        warn!("Stream ended with unparsed data in buffer: {}", buffer);
                    }
                    return None;
                }
            }
        }
    })
}

/// Try to parse a complete SSE event from the buffer
fn try_parse_buffer(buffer: &mut String) -> Option<Value> {
    // Look for complete SSE events (double newline terminated)
    let mut event_end = 0;
    let mut found_complete_event = false;
    
    // Find a complete SSE event (ends with \n\n or is at end of buffer)
    for (i, window) in buffer.as_bytes().windows(2).enumerate() {
        if window == b"\n\n" {
            event_end = i + 2;
            found_complete_event = true;
            break;
        }
    }
    
    // If no complete event found but buffer ends with single \n, wait for more
    if !found_complete_event {
        // Check if we have a potentially complete event at the end
        if buffer.ends_with("\ndata:") {
            // This is the start of a new event, wait for more
            return None;
        }
        if buffer.contains("data:") && buffer.ends_with("\n") {
            // Might be a complete event
            event_end = buffer.len();
            found_complete_event = true;
        } else {
            return None;
        }
    }
    
    if found_complete_event {
        let event_text = buffer[..event_end].to_string();
        buffer.drain(..event_end);
        
        // Parse the SSE event
        for line in event_text.lines() {
            let line = line.trim();
            
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            
            if line.starts_with("event:") {
                let event_type = line.strip_prefix("event:").unwrap_or("").trim();
                debug!("SSE event type: {}", event_type);
                continue;
            }
            
            if line.starts_with("data:") {
                let data_part = line.strip_prefix("data:").unwrap_or("").trim();
                
                if data_part == "[DONE]" {
                    debug!("Stream completed: [DONE] marker received");
                    continue;
                }
                
                if data_part.is_empty() {
                    continue;
                }
                
                // Check if this looks like JSON that might be incomplete
                if data_part.starts_with('{') {
                    // Quick heuristic: count braces to see if JSON is likely complete
                    let open_braces = data_part.chars().filter(|&c| c == '{').count();
                    let close_braces = data_part.chars().filter(|&c| c == '}').count();
                    if open_braces > close_braces {
                        // JSON is incomplete, put the event back and wait for more
                        *buffer = format!("{}\n{}", event_text, buffer);
                        return None;
                    }
                }
                
                // Try to parse as JSON
                match serde_json::from_str::<Value>(data_part) {
                    Ok(json_value) => {
                        return Some(json_value);
                    }
                    Err(e) => {
                        if data_part.starts_with('{') || data_part.starts_with('[') {
                            // This looked like JSON but failed to parse
                            // It might be split across chunks
                            warn!("Failed to parse SSE JSON: {} - Data preview: {:?}", 
                                  e, &data_part[..data_part.len().min(100)]);
                            
                            // Put it back in the buffer to accumulate more
                            *buffer = format!("{}\n{}", event_text, buffer);
                            return None;
                        }
                        // Not JSON, skip it
                        continue;
                    }
                }
            }
        }
    }
    
    None
}

/// Process GPT-5 response stream with GIGACHAD batching
pub fn process_gpt5_stream(
    mut stream: impl Stream<Item = Result<Value>> + Send + Unpin + 'static,
    _has_tools: bool,
    session_id: String,
    app_state: Arc<AppState>,
    project_id: Option<String>,
) -> impl Stream<Item = Result<ChatEvent>> + Send {
    // Shared state
    let buffer = Arc::new(std::sync::Mutex::new(String::new()));
    let metadata = Arc::new(std::sync::Mutex::new(StreamMetadata::default()));
    let completion_sent = Arc::new(std::sync::Mutex::new(false));
    
    // Create channel with reasonable bounded size to apply backpressure
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatEvent>>(100);
    
    // Spawn the processing task
    tokio::spawn(async move {
        // Batch accumulator
        let mut batch = String::new();
        let mut last_send = tokio::time::Instant::now();
        let mut chunk_count = 0;
        
        // GIGACHAD BATCHING PARAMETERS - GO BIG OR GO HOME
        const MAX_BATCH_SIZE: usize = 16384;  // 16KB - entire documents
        const MAX_BATCH_AGE_MS: u64 = 300;    // 300ms - still sub-second
        const MIN_BATCH_SIZE: usize = 256;    // Nothing tiny allowed
        
        info!("GIGACHAD batching activated: {}KB batches, {}ms timeout", 
              MAX_BATCH_SIZE / 1024, MAX_BATCH_AGE_MS);
        
        loop {
            // Use timeout to flush batches periodically
            let timeout_duration = Duration::from_secs(300); // 5 minute overall timeout
            match timeout(timeout_duration, stream.next()).await {
                Ok(Some(chunk_result)) => {
                    match chunk_result {
                        Ok(chunk) => {
                            chunk_count += 1;
                            
                            // Log first few chunks for debugging
                            if chunk_count <= 5 {
                                info!("RAW CHUNK #{}: {}", chunk_count, serde_json::to_string(&chunk).unwrap_or_default());
                            }
                            
                            if let Some(event_type) = chunk.get("type").and_then(|t| t.as_str()) {
                                debug!("Processing event #{} type: {}", chunk_count, event_type);
                                
                                match event_type {
                                    "response.output_text.delta" => {
                                        if let Some(delta) = chunk.get("delta").and_then(|d| d.as_str()) {
                                            info!("DELTA #{}: {:?} ({} chars)", chunk_count, delta, delta.len());
                                            
                                            // Update the complete buffer
                                            {
                                                let mut buf = buffer.lock().unwrap();
                                                buf.push_str(delta);
                                                info!("Buffer after delta #{}: {:?}", chunk_count, &*buf);
                                            }
                                            
                                            // Add to batch
                                            batch.push_str(delta);
                                            
                                            // With GIGACHAD batching, we only send when:
                                            // 1. We hit the massive size limit (16KB)
                                            // 2. The batch has been sitting for 300ms
                                            // 3. We get a clear signal to send (multiple newlines)
                                            let has_paragraph_break = batch.contains("\n\n");
                                            let should_send = 
                                                // Hit size limit (unlikely with normal responses)
                                                batch.len() >= MAX_BATCH_SIZE ||
                                                // Found natural break and batch is substantial
                                                (has_paragraph_break && batch.len() >= MIN_BATCH_SIZE) ||
                                                // Batch has aged out
                                                last_send.elapsed() > Duration::from_millis(MAX_BATCH_AGE_MS);
                                            
                                            if should_send && !batch.is_empty() {
                                                info!("GIGACHAD BATCH: Sending {} bytes", batch.len());
                                                
                                                // Send the batch
                                                if let Err(_) = tx.send(Ok(ChatEvent::Content {
                                                    text: batch.clone()
                                                })).await {
                                                    // Receiver dropped, exit
                                                    break;
                                                }
                                                batch.clear();
                                                last_send = tokio::time::Instant::now();
                                                
                                                // Yield to prevent overwhelming the receiver
                                                tokio::task::yield_now().await;
                                            }
                                        }
                                    }
                                    
                                    "response.output_text.done" => {
                                        info!("Text output complete");
                                        
                                        // Flush any remaining batch
                                        if !batch.is_empty() {
                                            info!("GIGACHAD FINAL: Flushing {} bytes", batch.len());
                                            let _ = tx.send(Ok(ChatEvent::Content {
                                                text: batch.clone()
                                            })).await;
                                            batch.clear();
                                        }
                                        
                                        // Get final buffer content
                                        let final_content = {
                                            let buf = buffer.lock().unwrap();
                                            buf.clone()
                                        };
                                        info!("Final buffer content: {:?}", final_content);
                                    }
                                    
                                    "response.done" => {
                                        info!("Response complete, sending completion events");
                                        
                                        // Send completion events if not already sent
                                        let should_send = {
                                            let mut sent = completion_sent.lock().unwrap();
                                            if !*sent {
                                                *sent = true;
                                                true
                                            } else {
                                                false
                                            }
                                        };
                                        
                                        if should_send {
                                            // Flush final batch if any
                                            if !batch.is_empty() {
                                                info!("GIGACHAD DONE: Final flush of {} bytes", batch.len());
                                                let _ = tx.send(Ok(ChatEvent::Content {
                                                    text: batch.clone()
                                                })).await;
                                            }
                                            
                                            let meta_clone = {
                                                let meta = metadata.lock().unwrap();
                                                meta.clone()
                                            };
                                            
                                            let _ = tx.send(Ok(ChatEvent::Complete {
                                                mood: meta_clone.mood.clone(),
                                                salience: meta_clone.salience,
                                                tags: meta_clone.tags.clone(),
                                            })).await;
                                            
                                            let _ = tx.send(Ok(ChatEvent::Done)).await;
                                            
                                            // Save to memory
                                            let buffer_content = buffer.lock().unwrap().clone();
                                            if let Err(e) = save_streaming_response(
                                                buffer_content,
                                                meta_clone,
                                                session_id.clone(),
                                                app_state.clone(),
                                                project_id.clone(),
                                            ).await {
                                                error!("Failed to save streaming response: {}", e);
                                            }
                                        }
                                        
                                        break; // Exit the loop
                                    }
                                    
                                    // Tool execution events
                                    "response.tool_call.function" => {
                                        if let Some(name) = chunk.get("name").and_then(|n| n.as_str()) {
                                            info!("Tool execution: {}", name);
                                            let _ = tx.send(Ok(ChatEvent::ToolExecution {
                                                tool_name: name.to_string(),
                                                status: "executing".to_string(),
                                            })).await;
                                        }
                                    }
                                    
                                    _ => {
                                        debug!("Unhandled event type: {}", event_type);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Stream chunk error: {}", e);
                            let _ = tx.send(Err(anyhow::anyhow!("Stream error: {}", e))).await;
                            break;
                        }
                    }
                }
                
                Ok(None) => {
                    info!("Stream ended");
                    
                    // Flush any remaining batch
                    if !batch.is_empty() {
                        info!("GIGACHAD EOF: Flushing final {} bytes", batch.len());
                        let _ = tx.send(Ok(ChatEvent::Content {
                            text: batch.clone()
                        })).await;
                    }
                    
                    // Send final events if not already sent
                    let should_send = {
                        let mut sent = completion_sent.lock().unwrap();
                        if !*sent {
                            *sent = true;
                            true
                        } else {
                            false
                        }
                    };
                    
                    if should_send {
                        let meta_clone = {
                            let meta = metadata.lock().unwrap();
                            meta.clone()
                        };
                        let _ = tx.send(Ok(ChatEvent::Complete {
                            mood: meta_clone.mood.clone(),
                            salience: meta_clone.salience,
                            tags: meta_clone.tags.clone(),
                        })).await;
                        let _ = tx.send(Ok(ChatEvent::Done)).await;
                    }
                    break;
                }
                
                Err(_) => {
                    error!("Stream timeout after 5 minutes");
                    let _ = tx.send(Err(anyhow::anyhow!("Stream timeout"))).await;
                    break;
                }
            }
            
            // Check if we need to flush batch due to age
            if !batch.is_empty() && last_send.elapsed() > Duration::from_millis(MAX_BATCH_AGE_MS) {
                info!("GIGACHAD TIMEOUT: Flushing aged batch of {} bytes", batch.len());
                if let Err(_) = tx.send(Ok(ChatEvent::Content {
                    text: batch.clone()
                })).await {
                    break;
                }
                batch.clear();
                last_send = tokio::time::Instant::now();
                tokio::task::yield_now().await;
            }
        }
        
        drop(tx); // Close the channel
    });
    
    // Convert receiver to stream with bounded buffer for backpressure
    ReceiverStream::new(rx)
}

#[derive(Debug, Clone, Default)]
struct StreamMetadata {
    mood: Option<String>,
    salience: Option<f32>,
    tags: Option<Vec<String>>,
    persona: Option<String>,
}

async fn save_streaming_response(
    content: String,
    metadata: StreamMetadata,
    session_id: String,
    app_state: Arc<AppState>,
    project_id: Option<String>,
) -> Result<()> {
    // Skip empty responses
    if content.trim().is_empty() {
        debug!("Skipping save of empty streaming response");
        return Ok(());
    }
    
    // Generate a summary (first 100 chars or so)
    let summary = if content.len() > 100 {
        let mut end = 100;
        while !content.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &content[..end])
    } else {
        content.to_string()
    };
    
    // Use actual metadata from stream, with sensible defaults only as fallback
    let response = crate::llm::types::ChatResponse {
        output: content.to_string(),
        persona: metadata.persona.unwrap_or_else(|| "Default".to_string()),
        mood: metadata.mood.unwrap_or_else(|| "neutral".to_string()),
        salience: metadata.salience.unwrap_or(5.0),
        summary,
        memory_type: "Response".to_string(),
        tags: metadata.tags.unwrap_or_else(|| vec!["assistant".to_string()]),
        intent: None,
        monologue: None,
        reasoning_summary: None,
    };
    
    app_state.memory_service.save_assistant_response(&session_id, &response, project_id.as_deref()).await?;
    
    if let Some(proj_id) = project_id {
        debug!("Assistant response saved with project context: {}", proj_id);
    }
    
    Ok(())
}

/// Check if streaming chunk indicates completion (GPT-5 specific)
pub fn is_completion_chunk(chunk: &Value) -> bool {
    // Check for GPT-5 response completion events
    if let Some(event_type) = chunk.get("type").and_then(|t| t.as_str()) {
        return event_type == "response.output_text.done" || 
               event_type == "response.done";
    }
    
    // Check for done field
    chunk.get("done").is_some()
}
