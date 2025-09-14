// src/llm/streaming/processor.rs
// Event processing for streaming responses from GPT-5 Responses API

use anyhow::Result;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::debug;

/// Events emitted while streaming
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text delta from the model
    Delta(String),
    /// Legacy text variant for compatibility
    Text(String),
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
    
    sse_stream.map(move |result| {
        match result {
            Ok(frame) => process_frame(
                frame, 
                &mut buffer, 
                &mut last_raw,
                structured_json,
                &json_announced
            ),
            Err(e) => Ok(StreamEvent::Error(e.to_string())),
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
    
    // Check event type
    if let Some(event_type) = frame.get("type").and_then(|t| t.as_str()) {
        match event_type {
            "text_delta" => handle_text_delta(frame, buffer, structured_json, json_announced),
            "content_part" => handle_content_part(frame, buffer),
            "response_done" => handle_done(buffer.clone(), last_raw.clone()),
            "error" => handle_error(frame),
            _ => {
                debug!("Unhandled event type: {}", event_type);
                Ok(StreamEvent::Delta(String::new()))
            }
        }
    } else {
        Ok(StreamEvent::Delta(String::new()))
    }
}

fn handle_text_delta(
    frame: Value,
    buffer: &mut String,
    structured_json: bool,
    json_announced: &Arc<AtomicBool>,
) -> Result<StreamEvent> {
    if let Some(delta) = frame.get("delta").and_then(|d| d.as_str()) {
        buffer.push_str(delta);
        
        if structured_json {
            // Only emit when JSON is complete
            if is_complete_json(buffer) && !json_announced.load(Ordering::SeqCst) {
                json_announced.store(true, Ordering::SeqCst);
                return Ok(StreamEvent::Delta(buffer.clone()));
            }
            Ok(StreamEvent::Delta(String::new()))
        } else {
            // Emit deltas immediately in text mode
            Ok(StreamEvent::Delta(delta.to_string()))
        }
    } else {
        Ok(StreamEvent::Delta(String::new()))
    }
}

fn handle_content_part(frame: Value, buffer: &mut String) -> Result<StreamEvent> {
    if let Some(part) = frame.get("content_part") {
        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            buffer.push_str(text);
            return Ok(StreamEvent::Text(text.to_string()));
        }
    }
    Ok(StreamEvent::Delta(String::new()))
}

fn handle_done(full_text: String, raw: Option<Value>) -> Result<StreamEvent> {
    Ok(StreamEvent::Done { full_text, raw })
}

fn handle_error(frame: Value) -> Result<StreamEvent> {
    let error_msg = frame.get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("Unknown streaming error")
        .to_string();
    Ok(StreamEvent::Error(error_msg))
}

fn is_complete_json(text: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok()
}
