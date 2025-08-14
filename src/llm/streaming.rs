// src/llm/streaming.rs
// Real streaming from OpenAI Responses SSE -> StreamEvent

use anyhow::Result;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::pin::Pin;
use tracing::debug;

use crate::llm::client::{extract_text_from_responses, OpenAIClient, ResponseStream};

/// Events emitted during a streaming response.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Incremental text delta (already decoded to UTF-8)
    Delta(String),
    /// Stream finished; includes the final text we accumulated (best-effort) and the last raw JSON frame (if any)
    Done {
        full_text: String,
        raw: Option<Value>,
    },
    /// Error surfaced from SSE / parsing
    Error(String),
}

/// Start a streaming response for a single user turn.
pub async fn start_response_stream(
    client: std::sync::Arc<OpenAIClient>,
    _session_id: String,
    user_text: String,
    _project_id: Option<String>,
    structured_json: bool,
) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
    use tracing::info;
    
    info!("🚀 Starting response stream - structured_json: {}", structured_json);
    
    // Build input for the Responses API
    let input = vec![serde_json::json!({
        "role": "user",
        "content": [{ "type": "input_text", "text": user_text }]
    })];

    // Build request with sanitized verbosity + reasoning effort
    let mut body = serde_json::json!({
        "model": client.model(),
        "input": input,
        "text": {
            "verbosity": sanitize_verbosity(client.verbosity())
        },
        "reasoning": {
            "effort": sanitize_reasoning(client.reasoning_effort())
        },
        "max_output_tokens": client.max_output_tokens(),
        "stream": true
    });

    if structured_json {
        // For GPT-5 Responses API, structured output format with schema
        body["text"]["format"] = serde_json::json!({
            "type": "json_schema",
            "name": "mira_response",
            "schema": {
                "type": "object",
                "properties": {
                    "output": {
                        "type": "string",
                        "description": "The main response text"
                    },
                    "mood": {
                        "type": "string",
                        "description": "The emotional tone of the response"
                    },
                    "salience": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 10,
                        "description": "Importance score from 1-10"
                    },
                    "summary": {
                        "type": "string",
                        "description": "Brief summary of the interaction"
                    },
                    "memory_type": {
                        "type": "string",
                        "enum": ["event", "fact", "emotion", "preference", "context"],
                        "description": "Category of memory"
                    },
                    "tags": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        },
                        "description": "Relevant tags for this interaction"
                    },
                    "intent": {
                        "type": "string",
                        "description": "The user's apparent intent"
                    },
                    "monologue": {
                        "type": ["string", "null"],
                        "description": "Internal reasoning or thoughts"
                    },
                    "reasoning_summary": {
                        "type": ["string", "null"],
                        "description": "Summary of reasoning process"
                    }
                },
                "required": ["output", "mood", "salience", "summary", "memory_type", "tags", "intent", "monologue", "reasoning_summary"],
                "additionalProperties": false
            },
            "strict": true
        });
    }

    info!("📤 Sending request to OpenAI Responses API");
    debug!("Request body: {}", serde_json::to_string_pretty(&body).unwrap_or_default());

    let sse: ResponseStream = client.post_response_stream(body).await?;
    info!("✅ SSE stream started successfully");

    // Keep an accumulator without holding a &mut across .await
    let acc = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let frame_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let first_frame_received = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mapped = sse
        .then({
            let acc = acc.clone();
            let frame_count = frame_count.clone();
            let first_frame_received = first_frame_received.clone();
            let is_structured = structured_json;
            move |item| {
                let acc = acc.clone();
                let frame_count = frame_count.clone();
                let first_frame_received = first_frame_received.clone();
                async move {
                    let count = frame_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    
                    if !first_frame_received.load(std::sync::atomic::Ordering::SeqCst) {
                        first_frame_received.store(true, std::sync::atomic::Ordering::SeqCst);
                        info!("📨 First SSE frame received!");
                    }
                    
                    match item {
                        Ok(v) => {
                            info!("📦 SSE frame #{}: {}", count, serde_json::to_string(&v).unwrap_or_default());
                            
                            // Check event type
                            let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            
                            match event_type {
                                "response.created" => {
                                    info!("🚀 Response created, ID: {}", 
                                        v.pointer("/response/id").and_then(|i| i.as_str()).unwrap_or("unknown"));
                                }
                                "response.in_progress" => {
                                    info!("⏳ Response in progress");
                                }
                                "response.output_item.added" => {
                                    info!("📝 Output item added");
                                    // Content will come in subsequent delta frames
                                }
                                "response.output_item.delta" => {
                                    // This is where the actual content comes
                                    if let Some(delta) = v.pointer("/delta/content").and_then(|c| c.as_str()) {
                                        info!("💬 Delta content: {}", delta);
                                        let mut guard = acc.lock().await;
                                        guard.push_str(delta);
                                        drop(guard);
                                        return Ok(StreamEvent::Delta(delta.to_string()));
                                    }
                                    // For structured JSON, it might be in /delta directly
                                    if let Some(delta_obj) = v.get("delta") {
                                        let delta_str = serde_json::to_string(delta_obj)?;
                                        info!("💬 Delta JSON: {}", delta_str);
                                        let mut guard = acc.lock().await;
                                        guard.push_str(&delta_str);
                                        drop(guard);
                                        return Ok(StreamEvent::Delta(delta_str));
                                    }
                                }
                                "response.output_item.done" => {
                                    info!("✅ Output item completed");
                                    // Item is complete, content should be in /item
                                    if let Some(item) = v.get("item") {
                                        if let Some(content) = item.get("content").and_then(|c| c.as_str()) {
                                            info!("📄 Complete item content: {}", content);
                                            return Ok(StreamEvent::Delta(content.to_string()));
                                        }
                                        // For structured JSON
                                        let item_str = serde_json::to_string(item)?;
                                        info!("📄 Complete item JSON: {}", item_str);
                                        return Ok(StreamEvent::Delta(item_str));
                                    }
                                }
                                "response.done" => {
                                    let full = { acc.lock().await.clone() };
                                    info!("✅ Response complete. Total text: {} chars", full.len());
                                    
                                    // For structured JSON, the final output might be in response.output
                                    if let Some(output) = v.pointer("/response/output").and_then(|o| o.as_array()) {
                                        if !output.is_empty() {
                                            // Get the first output item
                                            if let Some(first) = output.first() {
                                                if let Some(content) = first.get("content") {
                                                    let content_str = if content.is_string() {
                                                        content.as_str().unwrap().to_string()
                                                    } else {
                                                        serde_json::to_string(content)?
                                                    };
                                                    info!("📄 Final output content: {}", content_str);
                                                    return Ok(StreamEvent::Done {
                                                        full_text: content_str,
                                                        raw: Some(v),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                    
                                    return Ok(StreamEvent::Done {
                                        full_text: full,
                                        raw: Some(v),
                                    });
                                }
                                _ => {
                                    info!("⚠️ Unknown event type: {}", event_type);
                                }
                            }
                            
                            // If we haven't returned yet, this frame didn't have content
                            Ok(StreamEvent::Delta(String::new()))
                        }
                        Err(e) => {
                            info!("❌ SSE error at frame #{}: {:?}", count, e);
                            Ok(StreamEvent::Error(e.to_string()))
                        }
                    }
                }
            }
        })
        .filter_map(|res| async move {
            match &res {
                Ok(StreamEvent::Delta(s)) if s.is_empty() => None,
                _ => Some(res),
            }
        });

    Ok(Box::pin(mapped))
}

/// Normalize verbosity to allowed values
fn sanitize_verbosity(v: &str) -> &'static str {
    match v.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

/// Normalize reasoning effort to allowed values
fn sanitize_reasoning(v: &str) -> &'static str {
    match v.trim().to_ascii_lowercase().as_str() {
        "low" | "minimal" => "minimal",
        "high" => "high",
        _ => "medium",
    }
}

/// Try to pull a small text delta from a streaming Responses JSON frame
fn extract_output_text_delta(v: &Value) -> Option<String> {
    // For structured JSON responses, the entire JSON object might be streamed
    // Check if this is a complete JSON structure matching our schema
    if v.get("output").is_some() {
        // This looks like our complete structured response
        if let Some(output) = v.get("output").and_then(|o| o.as_str()) {
            debug!("✅ Found structured response output field");
            return Some(output.to_string());
        }
    }
    
    // For streaming structured JSON, OpenAI might send the JSON in chunks
    // Try to get the raw text representation
    if let Some(s) = v.as_str() {
        debug!("✅ Found raw string chunk");
        return Some(s.to_string());
    }
    
    // 1) { "delta": { "content": "..." } } - Standard streaming format
    if let Some(s) = v
        .get("delta")
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str())
    {
        return Some(s.to_string());
    }
    
    // 2) { "output_text": { "delta": "..." } }
    if let Some(s) = v
        .get("output_text")
        .and_then(|o| o.get("delta"))
        .and_then(|d| d.as_str())
    {
        return Some(s.to_string());
    }

    // 3) { "message": { "content": [ { "type":"output_text", "delta":"..." } ] } }
    if let Some(content) = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array())
    {
        for part in content {
            if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                if let Some(s) = part.get("delta").and_then(|d| d.as_str()) {
                    return Some(s.to_string());
                }
                if let Some(s) = part.get("text").and_then(|d| d.as_str()) {
                    return Some(s.to_string());
                }
            }
        }
    }

    // 4) { "choices": [{ "delta": { "content": "..." } }] } - Chat completions compat
    if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
        if let Some(first) = choices.first() {
            if let Some(s) = first
                .get("delta")
                .and_then(|d| d.get("content"))
                .and_then(|c| c.as_str())
            {
                return Some(s.to_string());
            }
        }
    }

    // 5) Top-level array of parts: [{ "type":"output_text", "delta":"..." }]
    if let Some(arr) = v.as_array() {
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                if let Some(s) = item.get("delta").and_then(|d| d.as_str()) {
                    return Some(s.to_string());
                }
                if let Some(s) = item.get("text").and_then(|d| d.as_str()) {
                    return Some(s.to_string());
                }
            }
        }
    }

    None
}
