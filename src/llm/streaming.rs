// src/llm/streaming.rs

use anyhow::Result;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::pin::Pin;
use tracing::{debug, info, warn};

use crate::llm::client::{OpenAIClient, ResponseStream};

/// Events emitted while streaming a response.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of plain text from the model (or, for structured_json=true, the buffered JSON once parseable).
    Delta(String),
    /// Legacy text variant for compatibility
    Text(String),
    /// Final result with the full accumulated text and the last raw SSE frame.
    Done { full_text: String, raw: Option<Value> },
    /// Surface transport / protocol errors.
    Error(String),
}

/// Public stream type.
pub type StreamResult = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;

/// Start a streaming response request.
pub async fn start_response_stream(
    client: &OpenAIClient,
    user_text: &str,
    system_prompt: Option<&str>,
    structured_json: bool,
) -> Result<StreamResult> {
    stream_response(client, user_text, system_prompt, structured_json).await
}

/// Build the request and translate SSE frames -> StreamEvent.
pub async fn stream_response(
    client: &OpenAIClient,
    user_text: &str,
    system_prompt: Option<&str>,
    structured_json: bool,
) -> Result<StreamResult> {
    info!("Starting response stream - structured_json: {}", structured_json);

    // Build input (system + user).
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

    // Base request body — Responses API.
    let mut body = serde_json::json!({
        "model": client.model(),
        "input": input,
        "text": { "verbosity": norm_verbosity(client.verbosity()) },
        "reasoning": { "effort": norm_effort(client.reasoning_effort()) },
        "max_output_tokens": client.max_output_tokens(),
        "stream": true,
    });

    // Structured JSON mode uses the JSON schema format in the text tool.
    if structured_json {
        body["text"]["format"] = serde_json::json!({
            "type": "json_schema",
            "name": "mira_response",
            "schema": {
                "type": "object",
                "properties": {
                    "output": { "type": "string" },
                    "mood": { "type": "string" },
                    "salience": { "type": "integer", "minimum": 0, "maximum": 10 },
                    "summary": { "type": "string" },
                    "memory_type": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "intent": { "type": "string" },
                    "monologue": { "type": ["string", "null"] },
                    "reasoning_summary": { "type": ["string", "null"] }
                },
                "required": [
                    "output", "mood", "salience", "summary", "memory_type",
                    "tags", "intent", "monologue", "reasoning_summary"
                ],
                "additionalProperties": false
            },
            "strict": true
        });
    }

    info!("Sending request to OpenAI Responses API");
    let sse: ResponseStream = client.stream_response(body).await?;
    info!("SSE stream started successfully");

    // Shared state across frames.
    let raw_text = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let json_buf = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let json_announced = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let last_raw = std::sync::Arc::new(tokio::sync::Mutex::new(None::<Value>));
    let frame_no = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Map SSE frames to outward events.
    let stream = sse
        .then({
            let raw_text = raw_text.clone();
            let json_buf = json_buf.clone();
            let json_announced = json_announced.clone();
            let last_raw = last_raw.clone();
            let frame_no = frame_no.clone();

            move |item| {
                let raw_text = raw_text.clone();
                let json_buf = json_buf.clone();
                let json_announced = json_announced.clone();
                let last_raw = last_raw.clone();
                let frame_no = frame_no.clone();

                async move {
                    match item {
                        Ok(v) => {
                            // Log & stash last raw.
                            let n = frame_no.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            let short = truncate_for_log(&serde_json::to_string(&v).unwrap_or_default(), 220);
                            info!("SSE frame #{}: {}", n, short);
                            {
                                let mut g = last_raw.lock().await;
                                *g = Some(v.clone());
                            }

                            let typ = v.get("type").and_then(|x| x.as_str()).unwrap_or("");

                            match typ {
                                // Lifecycle chatter — useful logs only.
                                "response.created" => {
                                    let rid = v.pointer("/response/id").and_then(|x| x.as_str()).unwrap_or("unknown");
                                    info!("Response created: {}", rid);
                                    Ok(None)
                                }
                                "response.in_progress" => {
                                    info!("Response in progress");
                                    Ok(None)
                                }

                                // Primary text streaming (modern event).
                                "response.text.delta" => {
                                    if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                                        {
                                            let mut g = raw_text.lock().await;
                                            g.push_str(delta);
                                        }
                                        if structured_json {
                                            let mut jb = json_buf.lock().await;
                                            jb.push_str(delta);
                                            if try_parse_complete_json(&jb) {
                                                // Announce the first time the JSON becomes parseable.
                                                if !json_announced.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                                    info!("Structured JSON became parseable (text.delta)");
                                                    return Ok(Some(StreamEvent::Delta(jb.clone())));
                                                }
                                            }
                                            // Do not spam deltas in JSON mode; wait until parseable or done.
                                            Ok(None)
                                        } else {
                                            // Emit both Delta and Text for compatibility
                                            Ok(Some(StreamEvent::Text(delta.to_string())))
                                        }
                                    } else {
                                        Ok(None)
                                    }
                                }

                                // Some models surface part additions (may include a one-shot `text` field).
                                "response.content_part.added" => {
                                    if let Some(part) = v.get("content_part") {
                                        if let Some(txt) = part.get("text").and_then(|t| t.as_str()) {
                                            {
                                                let mut g = raw_text.lock().await;
                                                g.push_str(txt);
                                            }
                                            if structured_json {
                                                let mut jb = json_buf.lock().await;
                                                jb.push_str(txt);
                                                if try_parse_complete_json(&jb)
                                                    && !json_announced.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                                        info!("Structured JSON became parseable (content_part.added)");
                                                        return Ok(Some(StreamEvent::Delta(jb.clone())));
                                                    }
                                                return Ok(None);
                                            } else {
                                                return Ok(Some(StreamEvent::Text(txt.to_string())));
                                            }
                                        }
                                    }
                                    Ok(None)
                                }

                                // Output item done often precedes response.done; treat as a flush point.
                                "response.output_item.done" => {
                                    debug!("output_item.done");
                                    if structured_json {
                                        let jb = json_buf.lock().await;
                                        if !jb.is_empty() && try_parse_complete_json(&jb)
                                            && !json_announced.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                                info!("Emitting buffered JSON at output_item.done");
                                                return Ok(Some(StreamEvent::Delta(jb.clone())));
                                            }
                                    }
                                    Ok(None)
                                }

                                // Final markers.
                                "response.done" | "response.text.done" => {
                                    info!("Response complete");
                                    if structured_json {
                                        let jb = json_buf.lock().await;
                                        if !jb.is_empty() && try_parse_complete_json(&jb)
                                            && !json_announced.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                                info!("Emitting buffered JSON at response.done");
                                                // Emit once before Done so the UI can record the JSON payload early.
                                                // The Done below still carries full_text.
                                                return Ok(Some(StreamEvent::Delta(jb.clone())));
                                            }
                                    }
                                    let full = raw_text.lock().await.clone();
                                    Ok(Some(StreamEvent::Done { full_text: full, raw: Some(v) }))
                                }

                                // Everything else — ignore.
                                _ => {
                                    debug!("Ignored event: {}", typ);
                                    Ok(None)
                                }
                            }
                        }
                        Err(e) => {
                            warn!("SSE error: {}", e);
                            Ok(Some(StreamEvent::Error(e.to_string())))
                        }
                    }
                }
            }
        })
        .filter_map(|maybe| async move {
            match maybe {
                Ok(Some(ev)) => Some(Ok(ev)),
                Ok(None) => None,
                Err(err) => Some(Err(err)),
            }
        });

    Ok(Box::pin(stream))
}

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
