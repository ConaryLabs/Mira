// src/llm/streaming/processor.rs
// Event processing for streaming responses from GPT-5 Responses API
use anyhow::Result;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, info, warn};

/// Events emitted while streaming
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text delta from the model
    Delta(String),
    /// Final result with full text
    Done { full_text: String, raw: Option<Value> },
    /// Error during streaming
    Error(String),
}

/// Process SSE stream into StreamEvents
pub fn process_stream(
    sse_stream: impl Stream<Item = Result<Value>> + Send + 'static,
    structured_json: bool,
) -> impl Stream<Item = Result<StreamEvent>> + Send {
    let mut buffer = String::new();
    let mut last_raw = None;
    let json_announced = Arc::new(AtomicBool::new(false));
    let mut frame_count = 0;
    
    sse_stream.map(move |result| {
        frame_count += 1;
        match result {
            Ok(frame) => {
                // Log the raw frame for debugging
                debug!("Frame #{}: {}", frame_count, serde_json::to_string(&frame).unwrap_or_default());
                process_frame(
                    frame, 
                    &mut buffer, 
                    &mut last_raw,
                    structured_json,
                    &json_announced
                )
            },
            Err(e) => {
                warn!("Stream error at frame #{}: {}", frame_count, e);
                Ok(StreamEvent::Error(e.to_string()))
            },
        }
    })
}

fn process_frame(
    frame: Value,
    buffer: &mut String,
    last_raw: &mut Option<Value>,
    structured_json: bool,
    json_announced: &Arc<AtomicBool>,
) -> Result<StreamEvent> {
    // Store raw frame
    *last_raw = Some(frame.clone());
    
    // GPT-5 Responses API uses different event types
    if let Some(event_type) = frame.get("type").and_then(|t| t.as_str()) {
        debug!("Processing event type: {}", event_type);
        
        match event_type {
            // Content block events (main text streaming)
            "content_block_start" => {
                // Content block is starting
                debug!("Content block started");
                Ok(StreamEvent::Delta(String::new()))
            },
            "content_block_delta" => {
                // This is where the actual text comes through
                if let Some(delta) = frame.get("delta") {
                    if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                        info!("Got text delta: {} chars", text.len());
                        buffer.push_str(text);
                        
                        if structured_json {
                            // Only emit when JSON is complete
                            if is_complete_json(buffer) && !json_announced.load(Ordering::SeqCst) {
                                json_announced.store(true, Ordering::SeqCst);
                                return Ok(StreamEvent::Delta(buffer.clone()));
                            }
                            Ok(StreamEvent::Delta(String::new()))
                        } else {
                            // Emit deltas immediately in text mode
                            Ok(StreamEvent::Delta(text.to_string()))
                        }
                    } else {
                        debug!("content_block_delta without text");
                        Ok(StreamEvent::Delta(String::new()))
                    }
                } else {
                    debug!("content_block_delta without delta field");
                    Ok(StreamEvent::Delta(String::new()))
                }
            },
            "content_block_stop" => {
                // Content block finished
                debug!("Content block stopped");
                Ok(StreamEvent::Delta(String::new()))
            },
            
            // Message-level events
            "message_start" => {
                // Message is starting
                debug!("Message started");
                Ok(StreamEvent::Delta(String::new()))
            },
            "message_delta" => {
                // Message metadata update (usage, etc)
                debug!("Message delta (metadata)");
                Ok(StreamEvent::Delta(String::new()))
            },
            "message_stop" => {
                // Message is complete
                info!("Message complete. Total buffer: {} chars", buffer.len());
                handle_done(buffer.clone(), last_raw.clone())
            },
            
            // Error events
            "error" => handle_error(frame),
            
            // Rate limit info
            "rate_limit" => {
                debug!("Rate limit info received");
                Ok(StreamEvent::Delta(String::new()))
            },
            
            // Ping (keepalive)
            "ping" => {
                debug!("Ping received");
                Ok(StreamEvent::Delta(String::new()))
            },
            
            // Unknown event type
            _ => {
                warn!("Unknown event type: {} - frame: {:?}", event_type, frame);
                // Try to extract any text content anyway
                if let Some(delta) = extract_any_text(&frame) {
                    buffer.push_str(&delta);
                    return Ok(StreamEvent::Delta(delta));
                }
                Ok(StreamEvent::Delta(String::new()))
            }
        }
    } else {
        // No type field - shouldn't happen with GPT-5 Responses API
        warn!("No 'type' field in frame: {:?}", frame);
        if let Some(text) = extract_any_text(&frame) {
            buffer.push_str(&text);
            return Ok(StreamEvent::Delta(text));
        }
        Ok(StreamEvent::Delta(String::new()))
    }
}

fn handle_done(full_text: String, raw: Option<Value>) -> Result<StreamEvent> {
    info!("Stream done. Total text: {} chars", full_text.len());
    if full_text.is_empty() {
        warn!("Stream completed but buffer is empty!");
    }
    Ok(StreamEvent::Done { full_text, raw })
}

fn handle_error(frame: Value) -> Result<StreamEvent> {
    let error_msg = if let Some(error) = frame.get("error") {
        if let Some(msg) = error.get("message").and_then(|m| m.as_str()) {
            msg.to_string()
        } else {
            error.to_string()
        }
    } else {
        "Unknown streaming error".to_string()
    };
    warn!("Stream error: {}", error_msg);
    Ok(StreamEvent::Error(error_msg))
}

fn is_complete_json(text: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok()
}

/// Try to extract text from various possible field locations
fn extract_any_text(frame: &Value) -> Option<String> {
    // Try common field names
    frame.get("text").and_then(|t| t.as_str())
        .or_else(|| frame.get("content").and_then(|c| c.as_str()))
        .or_else(|| frame.get("delta").and_then(|d| d.get("text")).and_then(|t| t.as_str()))
        .or_else(|| frame.get("message").and_then(|m| m.as_str()))
        .map(|s| s.to_string())
}
