// src/llm/streaming.rs
// Real streaming from OpenAI Responses SSE -> StreamEvent

use anyhow::{anyhow, Result};
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::pin::Pin;
use tracing::{debug, info, warn};

use crate::llm::client::{OpenAIClient, ResponseStream};

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

    let input = vec![
        serde_json::json!({
            "role": "system",
            "content": [{ "type": "input_text", "text": system_prompt.unwrap_or("") }]
        }),
        serde_json::json!({
            "role": "user",
            "content": [{ "type": "input_text", "text": user_text }]
        }),
    ];

    let mut body = serde_json::json!({
        "model": client.model(),
        "input": input,
        "text": { "verbosity": norm_verbosity(client.verbosity()) },
        "reasoning": { "effort": norm_effort(client.reasoning_effort()) },
        "max_output_tokens": client.max_output_tokens(),
        "stream": true,
    });

    if structured_json {
        body["text"]["format"] = serde_json::json!({
            "type": "json_schema",
            "name": "mira_response",
            "schema": {
                "type": "object",
                "properties": {
                    "output": { "type":"string" },
                    "mood": { "type":"string" },
                    "salience": { "type":"integer", "minimum": 0, "maximum": 10 },
                    "summary": { "type":"string" },
                    "memory_type": { "type":"string" },
                    "tags": { "type":"array", "items": { "type":"string" } },
                    "intent": { "type":"string" },
                    "monologue": { "type":["string","null"] },
                    "reasoning_summary": { "type":["string","null"] }
                },
                "required": ["output","mood","salience","summary","memory_type","tags","intent","monologue","reasoning_summary"],
                "additionalProperties": false
            },
            "strict": true
        });
    }

    info!("📤 Sending request to OpenAI Responses API");
    let sse: ResponseStream = client.stream_response(body).await?;
    info!("✅ SSE stream started successfully");

    let raw_text = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let json_buf = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let json_complete = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let last_raw = std::sync::Arc::new(tokio::sync::Mutex::new(None::<Value>));
    let frame_no = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let stream = sse
        .then({
            let raw_text = raw_text.clone();
            let json_buf = json_buf.clone();
            let json_complete = json_complete.clone();
            let last_raw = last_raw.clone();
            let frame_no = frame_no.clone();

            move |item| {
                let raw_text = raw_text.clone();
                let json_buf = json_buf.clone();
                let json_complete = json_complete.clone();
                let last_raw = last_raw.clone();
                let frame_no = frame_no.clone();

                async move {
                    match item {
                        Ok(v) => {
                            let n = frame_no.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            let short = serde_json::to_string(&v).unwrap_or_default();
                            info!("📦 SSE frame #{}: {}", n, truncate_for_log(&short, 200));

                            {
                                let mut g = last_raw.lock().await;
                                *g = Some(v.clone());
                            }

                            let typ = v.get("type").and_then(|x| x.as_str()).unwrap_or("");

                            match typ {
                                "response.created" => {
                                    info!("🚀 Response created: {}", v.pointer("/response/id").and_then(|x| x.as_str()).unwrap_or("unknown"));
                                }
                                "response.in_progress" => {
                                    info!("⏳ Response in progress");
                                }
                                "response.output_item.added" => {
                                    debug!("📝 output_item.added");
                                }
                                "response.content_part.added" => {
                                    debug!("🧩 content_part.added");
                                }
                                "response.output_text.delta" => {
                                    if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                                        {
                                            let mut g = raw_text.lock().await;
                                            g.push_str(delta);
                                        }
                                        if structured_json {
                                            let mut jb = json_buf.lock().await;
                                            jb.push_str(delta);
                                            if try_parse_complete_json(&jb) {
                                                if !json_complete.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                                    info!("📄 Complete structured JSON detected (from output_text.delta)");
                                                    return Ok(StreamEvent::Delta(jb.clone()));
                                                }
                                            }
                                        } else {
                                            return Ok(StreamEvent::Delta(delta.to_string()));
                                        }
                                    }
                                }
                                "response.message.delta" => {
                                    if let Some(parts) = v.get("delta").and_then(|d| d.get("content")).and_then(|c| c.as_array()) {
                                        let mut combined = String::new();
                                        for part in parts {
                                            if let Some(txt) = part.get("text").and_then(|t| t.get("value")).and_then(|x| x.as_str()) {
                                                combined.push_str(txt);
                                            } else if let Some(txt) = part.get("delta").and_then(|d| d.as_str()) {
                                                combined.push_str(txt);
                                            }
                                        }
                                        if !combined.is_empty() {
                                            {
                                                let mut g = raw_text.lock().await;
                                                g.push_str(&combined);
                                            }
                                            if structured_json {
                                                let mut jb = json_buf.lock().await;
                                                jb.push_str(&combined);
                                                if try_parse_complete_json(&jb) {
                                                    if !json_complete.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                                        info!("📄 Complete structured JSON detected (from message.delta)");
                                                        return Ok(StreamEvent::Delta(jb.clone()));
                                                    }
                                                }
                                            } else {
                                                return Ok(StreamEvent::Delta(combined));
                                            }
                                        }
                                    }
                                }
                                "response.output_text.done" | "response.content_part.done" => {
                                    debug!("✅ text/content part done");
                                }
                                "response.output_item.done" => {
                                    info!("✅ Output item completed");
                                    if structured_json {
                                        let jb = json_buf.lock().await;
                                        if !jb.is_empty() && try_parse_complete_json(&jb) {
                                            if !json_complete.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                                info!("📤 Emitting buffered JSON at output_item.done");
                                                return Ok(StreamEvent::Delta(jb.clone()));
                                            }
                                        }
                                    }
                                    let full = raw_text.lock().await.clone();
                                    return Ok(StreamEvent::Done { full_text: full, raw: Some(v) });
                                }
                                "response.done" => {
                                    info!("🎉 Response complete");
                                    if structured_json {
                                        let jb = json_buf.lock().await;
                                        if !jb.is_empty() && try_parse_complete_json(&jb) {
                                            if !json_complete.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                                info!("📤 Emitting buffered JSON at response.done");
                                                return Ok(StreamEvent::Delta(jb.clone()));
                                            }
                                        }
                                    }
                                    let full = raw_text.lock().await.clone();
                                    return Ok(StreamEvent::Done { full_text: full, raw: Some(v) });
                                }
                                _ => {
                                    debug!("📋 Other event type: {}", typ);
                                }
                            }

                            Err(anyhow!("no outward event in this frame"))
                        }
                        Err(e) => {
                            warn!("❌ SSE error: {}", e);
                            Ok(StreamEvent::Error(e.to_string()))
                        }
                    }
                }
            }
        })
        .filter_map(|r| async move {
            match r {
                Ok(ev) => Some(Ok(ev)),
                Err(_) => None,
            }
        });

    Ok(Box::pin(stream))
}

// === strict literal normalizers ===

fn norm_verbosity(v: &str) -> &'static str {
    match v.to_ascii_lowercase().as_str() {
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }
}

fn norm_effort(r: &str) -> &'static str {
    match r.to_ascii_lowercase().as_str() {
        "minimal" => "minimal",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }
}

fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

fn try_parse_complete_json(buf: &str) -> bool {
    serde_json::from_str::<Value>(buf).is_ok()
}
