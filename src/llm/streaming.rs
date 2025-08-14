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
    // Minimal streaming body for the Responses API.
    let input = vec![serde_json::json!({
        "role": "user",
        "content": [{ "type": "input_text", "text": user_text }]
    })];

    // Build request with **sanitized** verbosity + reasoning.effort.
    let mut body = serde_json::json!({
        "model": client.model(),
        "input": input,
        "text": { "verbosity": sanitize_verbosity(client.verbosity()) },
        "reasoning": { "effort": sanitize_reasoning(client.reasoning_effort()) },
        "max_output_tokens": client.max_output_tokens(),
        "stream": true
    });

    if structured_json {
        body["text"]["format"] = serde_json::json!({ "type": "json_object" });
    }

    let sse: ResponseStream = client.post_response_stream(body).await?;

    // Keep an accumulator without holding a &mut across .await.
    let acc = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));

    let mapped = sse
        .then({
            let acc = acc.clone();
            move |item| {
                let acc = acc.clone();
                async move {
                    match item {
                        Ok(v) => {
                            // Terminal frame?
                            if v.get("done").and_then(|b| b.as_bool()) == Some(true) {
                                let full = { acc.lock().await.clone() };
                                return Ok(StreamEvent::Done {
                                    full_text: full,
                                    raw: Some(v),
                                });
                            }

                            // Extract incremental text tokens from common streaming shapes.
                            if let Some(delta) = extract_output_text_delta(&v) {
                                let mut guard = acc.lock().await;
                                guard.push_str(&delta);
                                drop(guard);
                                return Ok(StreamEvent::Delta(delta));
                            }

                            // Sometimes a frame contains a full text fragment — treat as a delta.
                            if let Some(full_txt) = extract_text_from_responses(&v) {
                                let mut guard = acc.lock().await;
                                guard.push_str(&full_txt);
                                drop(guard);
                                return Ok(StreamEvent::Delta(full_txt));
                            }

                            // Unknown/unsupported frame — ignore
                            debug!("(stream) unrecognized SSE frame: {}", v);
                            Ok(StreamEvent::Delta(String::new()))
                        }
                        Err(e) => Ok(StreamEvent::Error(e.to_string())),
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

/// Normalize verbosity to allowed values.
fn sanitize_verbosity(v: &str) -> &'static str {
    match v.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

/// Normalize reasoning effort to allowed values.
fn sanitize_reasoning(v: &str) -> &'static str {
    match v.trim().to_ascii_lowercase().as_str() {
        "low" | "minimal" => "low",
        "high" => "high",
        _ => "medium",
    }
}

/// Try to pull a small text delta from a streaming Responses JSON frame.
fn extract_output_text_delta(v: &Value) -> Option<String> {
    // 1) { "output_text": { "delta": "..." } }
    if let Some(s) = v
        .get("output_text")
        .and_then(|o| o.get("delta"))
        .and_then(|d| d.as_str())
    {
        return Some(s.to_string());
    }

    // 2) { "message": { "content": [ { "type":"output_text", "delta":"..." } ] } }
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

    // 3) Top-level array of parts: [{ "type":"output_text", "delta":"..." }]
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
