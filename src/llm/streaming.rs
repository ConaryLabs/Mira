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
///
/// Signature is aligned with `ws/chat.rs`:
///     start_response_stream(client, session_id, user_text, project_id, structured_json)
///
/// - `structured_json = false` streams plain text deltas.
/// - If `true`, we still stream text tokens, but you can extend the parser below to surface JSON parts.
pub async fn start_response_stream(
    client: std::sync::Arc<OpenAIClient>,
    _session_id: String,
    user_text: String,
    _project_id: Option<String>,
    structured_json: bool,
) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
    // Minimal streaming body for the Responses API.
    // IMPORTANT: do NOT send a top-level `parameters` object.
    let input = vec![serde_json::json!({
        "role": "user",
        "content": [{ "type": "input_text", "text": user_text }]
    })];

    let mut body = serde_json::json!({
        "model": client.model(),
        "input": input,
        "verbosity": client.verbosity(),
        "reasoning": { "effort": client.reasoning_effort() },
        "max_output_tokens": client.max_output_tokens(),
        "stream": true
    });

    if structured_json {
        // Enforce JSON output via the official field.
        body.as_object_mut()
            .unwrap()
            .insert("response_format".to_string(), serde_json::json!({ "type": "json_object" }));
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

                            // Unknown/unsupported frame — ignore (emit no-op delta for visibility, then drop it below)
                            debug!("(stream) unrecognized SSE frame: {}", v);
                            Ok(StreamEvent::Delta(String::new()))
                        }
                        Err(e) => Ok(StreamEvent::Error(e.to_string())),
                    }
                }
            }
        })
        // Drop empty no-op deltas so the client only sees meaningful chunks.
        .filter_map(|res| async move {
            match &res {
                Ok(StreamEvent::Delta(s)) if s.is_empty() => None,
                _ => Some(res),
            }
        });

    Ok(Box::pin(mapped))
}

/// Try to pull a small text delta from a streaming Responses JSON frame.
/// Covers several common shapes.
///
/// Supported examples:
/// 1) { "output_text": { "delta": "..." } }
/// 2) { "message": { "content": [ { "type":"output_text", "delta":"..." } ] } }
///    (and it may send "text" instead of "delta")
/// 3) Top-level array: [ { "type":"output_text", "delta":"..." } ]
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
