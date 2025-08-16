// src/llm/streaming.rs
// Real streaming from OpenAI Responses SSE -> StreamEvent

use anyhow::Result;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::pin::Pin;
use tracing::{debug, info};

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

    // Keep an accumulator without holding a &mut across .await
    let acc = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let frame_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let stream = sse
        .then({
            let acc = acc.clone();
            let frame_count = frame_count.clone();
            move |item| {
                let acc = acc.clone();
                let frame_count = frame_count.clone();
                async move {
                    match item {
                        Ok(v) => {
                            let count = frame_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
                                    // Old shape (some models/tools still emit)
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

                                // 🚨 NEW GPT‑5 Responses streaming shapes
                                "response.content_part.added" => {
                                    info!("🧩 Content part added");
                                    // No-op: part scaffolding; actual text arrives via response.output_text.delta
                                }
                                "response.content_part.done" => {
                                    info!("🧩 Content part done");
                                }
                                "response.output_text.delta" => {
                                    // New primary text delta for Responses API
                                    if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                                        info!("💬 Output text delta: {}", delta);
                                        let mut guard = acc.lock().await;
                                        guard.push_str(delta);
                                        drop(guard);
                                        return Ok(StreamEvent::Delta(delta.to_string()));
                                    }
                                    // Fallback nested shape
                                    if let Some(s) = v.pointer("/output_text/delta").and_then(|d| d.as_str()) {
                                        info!("💬 Output text delta (nested): {}", s);
                                        let mut guard = acc.lock().await;
                                        guard.push_str(s);
                                        drop(guard);
                                        return Ok(StreamEvent::Delta(s.to_string()));
                                    }
                                }
                                "response.output_text.done" => {
                                    info!("✅ Output text done");
                                }
                                "response.message.delta" => {
                                    // Some models stream message-level deltas; accumulate if present
                                    if let Some(s) = v.pointer("/delta/content").and_then(|c| c.as_str()) {
                                        let mut guard = acc.lock().await;
                                        guard.push_str(s);
                                        drop(guard);
                                        return Ok(StreamEvent::Delta(s.to_string()));
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

                                    // Otherwise return the accumulated stream text
                                    return Ok(StreamEvent::Done {
                                        full_text: full,
                                        raw: Some(v),
                                    });
                                }
                                _ => {
                                    info!("⚠️ Unknown event type: {}", event_type);
                                }
                            }

                            // If we haven't returned yet, this frame
                            // doesn't produce a delta for the consumer.
                            Ok(StreamEvent::Delta(String::new()))
                        }
                        Err(e) => {
                            // Use current frame count for error context
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
        "low" | "medium" | "high" | "minimal" => serde_json::json!(match v {
            "minimal" => "minimal",
            _ => v,
        }),
        _ => serde_json::json!("medium"),
    }
}
