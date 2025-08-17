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

    let sse: ResponseStream = client.post_response_stream(body).await?;
    info!("✅ SSE stream started successfully");

    // Keep accumulators for both raw JSON and structured content
    let raw_acc = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let json_acc = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let frame_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let is_structured = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(structured_json));
    let complete_json_sent = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let stream = sse
        .then({
            let raw_acc = raw_acc.clone();
            let json_acc = json_acc.clone();
            let frame_count = frame_count.clone();
            let is_structured = is_structured.clone();
            let complete_json_sent = complete_json_sent.clone();
            
            move |item| {
                let raw_acc = raw_acc.clone();
                let json_acc = json_acc.clone();
                let frame_count = frame_count.clone();
                let is_structured = is_structured.clone();
                let complete_json_sent = complete_json_sent.clone();
                
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
                                "response.output_text.delta" => {
                                    // This is where the structured JSON comes through
                                    if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                                        info!("💬 Output text delta: {}", delta);
                                        
                                        // Accumulate the JSON string
                                        let mut json_guard = json_acc.lock().await;
                                        json_guard.push_str(delta);
                                        
                                        // For structured JSON, don't send individual deltas
                                        // We'll send the complete parsed output later
                                        if is_structured.load(std::sync::atomic::Ordering::SeqCst) {
                                            drop(json_guard);
                                            // Return empty delta to filter out
                                            return Ok(StreamEvent::Delta(String::new()));
                                        } else {
                                            // For non-structured, accumulate and send
                                            let mut raw_guard = raw_acc.lock().await;
                                            raw_guard.push_str(delta);
                                            drop(raw_guard);
                                            drop(json_guard);
                                            return Ok(StreamEvent::Delta(delta.to_string()));
                                        }
                                    }
                                }
                                "response.output_text.done" => {
                                    info!("✅ Output text done");
                                    
                                    // For structured JSON, send the complete JSON as one chunk
                                    if is_structured.load(std::sync::atomic::Ordering::SeqCst) 
                                        && !complete_json_sent.load(std::sync::atomic::Ordering::SeqCst) {
                                        
                                        let json_str = json_acc.lock().await.clone();
                                        if !json_str.is_empty() {
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
                                            info!("📄 Sending complete structured JSON: {}", preview);
                                            
                                            // Mark as sent
                                            complete_json_sent.store(true, std::sync::atomic::Ordering::SeqCst);
                                            
                                            // Send the complete JSON for the WebSocket handler to parse
                                            return Ok(StreamEvent::Delta(json_str));
                                        }
                                    }
                                }
                                "response.output_item.done" => {
                                    info!("✅ Output item completed");
                                    
                                    // Handle the complete item
                                    if let Some(item) = v.get("item") {
                                        // Check for content array (structured response)
                                        if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                                            for content_item in content_array {
                                                if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                                                    // Safe string truncation for logging
                                                    let preview = if text.len() > 200 {
                                                        let mut end = 200;
                                                        while !text.is_char_boundary(end) && end > 0 {
                                                            end -= 1;
                                                        }
                                                        format!("{}...", &text[..end])
                                                    } else {
                                                        text.to_string()
                                                    };
                                                    info!("📄 Complete item text: {}", preview);
                                                    
                                                    // For structured JSON, send the complete JSON
                                                    if is_structured.load(std::sync::atomic::Ordering::SeqCst) {
                                                        complete_json_sent.store(true, std::sync::atomic::Ordering::SeqCst);
                                                        
                                                        // Try to parse and extract output for raw accumulator
                                                        if let Ok(structured) = serde_json::from_str::<Value>(text) {
                                                            if let Some(output) = structured.get("output").and_then(|o| o.as_str()) {
                                                                let mut raw_guard = raw_acc.lock().await;
                                                                raw_guard.push_str(output);
                                                                drop(raw_guard);
                                                            }
                                                        }
                                                        
                                                        return Ok(StreamEvent::Delta(text.to_string()));
                                                    } else {
                                                        // Non-structured, send as-is
                                                        let mut raw_guard = raw_acc.lock().await;
                                                        raw_guard.push_str(text);
                                                        drop(raw_guard);
                                                        return Ok(StreamEvent::Delta(text.to_string()));
                                                    }
                                                }
                                            }
                                        }
                                        // Simple content string
                                        else if let Some(content) = item.get("content").and_then(|c| c.as_str()) {
                                            info!("📄 Complete item content: {}", content);
                                            let mut raw_guard = raw_acc.lock().await;
                                            raw_guard.push_str(content);
                                            drop(raw_guard);
                                            return Ok(StreamEvent::Delta(content.to_string()));
                                        }
                                    }
                                }
                                "response.done" | "response.completed" => {
                                    let full = raw_acc.lock().await.clone();
                                    let json_full = json_acc.lock().await.clone();
                                    
                                    info!("✅ Response complete. Total text: {} chars, JSON: {} chars", 
                                        full.len(), json_full.len());

                                    // If we have structured JSON that wasn't sent yet, send it now
                                    if is_structured.load(std::sync::atomic::Ordering::SeqCst) 
                                        && !complete_json_sent.load(std::sync::atomic::Ordering::SeqCst) 
                                        && !json_full.is_empty() {
                                        
                                        // Try to parse and extract output
                                        if let Ok(structured) = serde_json::from_str::<Value>(&json_full) {
                                            if let Some(output) = structured.get("output").and_then(|o| o.as_str()) {
                                                return Ok(StreamEvent::Done {
                                                    full_text: json_full, // Send full JSON for metadata
                                                    raw: Some(v),
                                                });
                                            }
                                        }
                                    }

                                    // Use the raw accumulator for Done event
                                    return Ok(StreamEvent::Done {
                                        full_text: if !full.is_empty() { full } else { json_full },
                                        raw: Some(v),
                                    });
                                }
                                _ => {
                                    warn!("⚠️ Unknown event type: {}", event_type);
                                }
                            }

                            // If we haven't returned yet, this frame doesn't produce a delta
                            Ok(StreamEvent::Delta(String::new()))
                        }
                        Err(e) => {
                            let current = frame_count.load(std::sync::atomic::Ordering::SeqCst);
                            info!("❌ SSE error at frame #{}: {:?}", current, e);
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

    Ok(Box::pin(stream))
}

fn sanitize_verbosity(v: &str) -> Value {
    match v {
        "low" | "medium" | "high" => serde_json::json!(v),
        _ => serde_json::json!("medium"),
    }
}

fn sanitize_reasoning(v: &str) -> Value {
    match v {
        "minimal" | "low" | "medium" | "high" => serde_json::json!(v),
        _ => serde_json::json!("medium"),
    }
}
