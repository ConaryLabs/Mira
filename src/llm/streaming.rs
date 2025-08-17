// src/llm/streaming.rs
// Real streaming from OpenAI Responses SSE -> StreamEvent

use anyhow::Result;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::pin::Pin;
use tracing::{debug, info, warn};

use crate::llm::client::{OpenAIClient, ResponseStream};

/// Events emitted during a streaming response.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Delta(String),
    Done {
        full_text: String,
        raw: Option<Value>,
    },
    Error(String),
}

pub type StreamResult = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;

/// Back-compat shim: older WS code calls `start_response_stream`.
/// Delegate to `stream_response`.
pub async fn start_response_stream(
    client: &OpenAIClient,
    user_text: &str,
    system_prompt: Option<&str>,
    structured_json: bool,
) -> Result<StreamResult> {
    stream_response(client, user_text, system_prompt, structured_json).await
}

pub async fn stream_response(
    client: &OpenAIClient,
    user_text: &str,
    system_prompt: Option<&str>,
    structured_json: bool,
) -> Result<StreamResult> {
    info!("🚀 Starting response stream - structured_json: {}", structured_json);

    // Build input for the Responses API (content parts ready)
    let input = vec![
        serde_json::json!({"role":"system","content": system_prompt.unwrap_or("")}),
        serde_json::json!({"role":"user","content": user_text})
    ];

    let mut body = serde_json::json!({
        "model": client.model(),
        "input": input,
        // Temperature intentionally omitted — default server-side.
        "parallel_tool_calls": true,
        "top_p": 1.0,
        "top_logprobs": 0,
        "truncation": "disabled",
        "service_tier": "auto",
        "store": true,
        "metadata": {},
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
                        "enum": ["event","fact","emotion","preference","context"],
                        "description": "Category of memory"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
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

    // Fixed: Use stream_response instead of post_response_stream
    let sse: ResponseStream = client.stream_response(body).await?;
    info!("✅ SSE stream started successfully");

    // Keep accumulators for both raw JSON and structured content
    let raw_acc = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let json_acc = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let frame_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let is_structured = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(structured_json));
    let complete_json_sent = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let final_json = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));

    let stream = sse
        .then({
            let raw_acc = raw_acc.clone();
            let json_acc = json_acc.clone();
            let frame_count = frame_count.clone();
            let is_structured = is_structured.clone();
            let complete_json_sent = complete_json_sent.clone();
            let final_json = final_json.clone();
            
            move |item| {
                let raw_acc = raw_acc.clone();
                let json_acc = json_acc.clone();
                let frame_count = frame_count.clone();
                let is_structured = is_structured.clone();
                let complete_json_sent = complete_json_sent.clone();
                let final_json = final_json.clone();
                
                async move {
                    match item {
                        Ok(v) => {
                            let count = frame_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            
                            // Safe string truncation for logging
                            let log_str = if let Ok(s) = serde_json::to_string(&v) {
                                if s.len() > 200 {
                                    let mut end = 200;
                                    while !s.is_char_boundary(end) && end > 0 {
                                        end -= 1;
                                    }
                                    format!("{}...", &s[..end])
                                } else {
                                    s
                                }
                            } else {
                                "parse error".to_string()
                            };
                            info!("📦 SSE frame #{}: {}", count, log_str);

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
                                }
                                "response.content_part.added" => {
                                    info!("🧩 Content part added");
                                }
                                "response.content_part.done" => {
                                    info!("✅ Content part completed");
                                }
                                "response.output_text.delta" => {
                                    // This is where the structured JSON comes through
                                    if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                                        info!("📝 Text delta: {} chars", delta.len());
                                        
                                        let mut raw_guard = raw_acc.lock().await;
                                        raw_guard.push_str(delta);
                                        
                                        if is_structured.load(std::sync::atomic::Ordering::SeqCst) {
                                            let mut json_guard = json_acc.lock().await;
                                            json_guard.push_str(delta);
                                            
                                            // Try to parse accumulated JSON
                                            if let Ok(_parsed) = serde_json::from_str::<Value>(&*json_guard) {
                                                // Store the complete JSON for later
                                                let mut final_guard = final_json.lock().await;
                                                *final_guard = json_guard.clone();
                                                
                                                // Only send once when complete
                                                if !complete_json_sent.load(std::sync::atomic::Ordering::SeqCst) {
                                                    let json_str = json_guard.clone();
                                                    
                                                    // Safe string truncation for logging
                                                    let preview = if json_str.len() > 200 {
                                                        let mut end = 200;
                                                        while !json_str.is_char_boundary(end) && end > 0 {
                                                            end -= 1;
                                                        }
                                                        format!("{}...", &json_str[..end])
                                                    } else {
                                                        json_str.clone()
                                                    };
                                                    info!("📄 Complete structured JSON ready: {}", preview);
                                                    
                                                    // Mark as sent
                                                    complete_json_sent.store(true, std::sync::atomic::Ordering::SeqCst);
                                                    
                                                    // Send the complete JSON
                                                    return Ok(StreamEvent::Delta(json_str));
                                                }
                                            }
                                        } else {
                                            // For non-structured, send the delta immediately
                                            return Ok(StreamEvent::Delta(delta.to_string()));
                                        }
                                    }
                                }
                                "response.output_item.done" => {
                                    info!("✅ Output item completed");
                                    
                                    // If we have structured JSON ready, make sure it gets sent
                                    if is_structured.load(std::sync::atomic::Ordering::SeqCst) {
                                        let final_guard = final_json.lock().await;
                                        if !final_guard.is_empty() && !complete_json_sent.load(std::sync::atomic::Ordering::SeqCst) {
                                            complete_json_sent.store(true, std::sync::atomic::Ordering::SeqCst);
                                            info!("📤 Sending complete JSON from output_item.done");
                                            return Ok(StreamEvent::Delta(final_guard.clone()));
                                        }
                                    }
                                    
                                    // Also send a Done event here since response.done might not come
                                    let raw_guard = raw_acc.lock().await;
                                    let full_text = raw_guard.clone();
                                    
                                    info!("🎉 Sending Done event from output_item.done (fallback)");
                                    return Ok(StreamEvent::Done {
                                        full_text,
                                        raw: Some(v),
                                    });
                                }
                                "response.done" => {
                                    info!("🎉 Response complete - sending Done event!");
                                    
                                    let raw_guard = raw_acc.lock().await;
                                    let full_text = raw_guard.clone();
                                    
                                    info!("📊 Total streamed: {} chars", full_text.len());
                                    
                                    // ALWAYS return the Done event
                                    return Ok(StreamEvent::Done {
                                        full_text,
                                        raw: Some(v),
                                    });
                                }
                                _ => {
                                    debug!("📋 Other event type: {}", event_type);
                                }
                            }
                            
                            // No event to emit for this frame
                            Err(anyhow::anyhow!("No stream event"))
                        }
                        Err(e) => {
                            warn!("❌ Stream error: {}", e);
                            Ok(StreamEvent::Error(e.to_string()))
                        }
                    }
                }
            }
        })
        .filter_map(|result| async move {
            match result {
                Ok(event) => {
                    // Always pass through Delta, Done, and Error events
                    Some(Ok(event))
                },
                Err(_) => None, // Skip non-events (the intermediate frames)
            }
        });

    Ok(Box::pin(stream))
}

/// Helper functions for sanitizing parameters
fn sanitize_verbosity(v: &str) -> &str {
    match v.to_lowercase().as_str() {
        "low" | "medium" | "high" => v,
        _ => "medium"
    }
}

fn sanitize_reasoning(r: &str) -> &str {
    match r.to_lowercase().as_str() {
        "minimal" | "medium" | "high" => r,
        _ => "medium"
    }
}
