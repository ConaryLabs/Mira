//! Streaming and sync chat handlers

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        Json,
    },
};
use futures::stream::Stream;
use std::convert::Infallible;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::chat::process_chat;
use super::types::{
    ChatEvent, ChatRequest, MessageBlock, SyncChatResponse, SyncErrorResponse,
    ToolCallResultData, UsageInfo,
};
use super::AppState;
use crate::core::ops::audit::{AuditEvent, AuditEventType, AuditSource};

/// SSE streaming chat endpoint
pub async fn chat_stream_handler(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<ChatEvent>(100);

    // Spawn the chat processing task
    tokio::spawn(async move {
        if let Err(e) = process_chat(state, request, tx.clone()).await {
            let _ = tx
                .send(ChatEvent::Error {
                    message: e.to_string(),
                })
                .await;
        }
        let _ = tx.send(ChatEvent::Done).await;
    });

    // Convert channel to SSE stream
    let stream = async_stream::stream! {
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            let data = serde_json::to_string(&event).unwrap_or_default();
            yield Ok(Event::default().data(data));
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Custom error type that returns structured JSON
pub struct SyncError {
    status: StatusCode,
    request_id: String,
    timestamp: i64,
    message: String,
}

impl SyncError {
    pub fn new(status: StatusCode, request_id: String, timestamp: i64, message: impl Into<String>) -> Self {
        Self { status, request_id, timestamp, message: message.into() }
    }
}

impl axum::response::IntoResponse for SyncError {
    fn into_response(self) -> axum::response::Response {
        let body = SyncErrorResponse {
            request_id: self.request_id,
            timestamp: self.timestamp,
            success: false,
            error: self.message,
        };
        (self.status, Json(body)).into_response()
    }
}

/// Max message size for sync endpoint (32KB)
const SYNC_MAX_MESSAGE_BYTES: usize = 32 * 1024;

/// Non-streaming chat endpoint for programmatic access (e.g., Claude calling Mira)
///
/// Rate limiting: Uses semaphore for concurrency gating (max N concurrent requests),
/// NOT a true rate limiter (requests/sec). For rate limiting, use token bucket at proxy layer.
pub async fn chat_sync_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChatRequest>,
) -> Result<Json<SyncChatResponse>, SyncError> {
    let request_id = Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().timestamp();

    // Check auth token if configured
    if let Some(ref expected_token) = state.sync_token {
        let auth_header = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let provided_token = auth_header.strip_prefix("Bearer ").unwrap_or("");
        if provided_token != expected_token {
            // Log without echoing the provided token
            tracing::warn!(
                request_id = %request_id,
                "Sync endpoint auth failure: invalid or missing token"
            );
            // Audit log (async, fire-and-forget)
            if let Some(db) = &state.db {
                let db = db.clone();
                let req_id = request_id.clone();
                tokio::spawn(async move {
                    let _ = crate::core::ops::audit::log_audit(
                        &db,
                        AuditEvent::new(AuditEventType::AuthFailure, AuditSource::Sync)
                            .request_id(req_id)
                            .details(serde_json::json!({"reason": "invalid_or_missing_token"}))
                            .warn(),
                    ).await;
                });
            }
            return Err(SyncError::new(
                StatusCode::UNAUTHORIZED,
                request_id,
                timestamp,
                "Invalid or missing sync token",
            ));
        }
    }

    // Size limit check
    if request.message.len() > SYNC_MAX_MESSAGE_BYTES {
        tracing::warn!(
            request_id = %request_id,
            size = request.message.len(),
            max = SYNC_MAX_MESSAGE_BYTES,
            "Sync endpoint rejected: message too large"
        );
        return Err(SyncError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            request_id,
            timestamp,
            format!("Message exceeds {} byte limit", SYNC_MAX_MESSAGE_BYTES),
        ));
    }

    // Concurrency gating: acquire permit or reject (not a rate limiter - see doc comment)
    let semaphore = state.sync_semaphore.clone();
    let _permit = match semaphore.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            tracing::warn!(
                request_id = %request_id,
                "Sync endpoint rejected: too many concurrent requests"
            );
            // Audit log (async, fire-and-forget)
            if let Some(db) = &state.db {
                let db = db.clone();
                let req_id = request_id.clone();
                let proj = request.project_path.clone();
                tokio::spawn(async move {
                    let _ = crate::core::ops::audit::log_audit(
                        &db,
                        AuditEvent::new(AuditEventType::RateLimited, AuditSource::Sync)
                            .request_id(req_id)
                            .project(proj)
                            .warn(),
                    ).await;
                });
            }
            return Err(SyncError::new(
                StatusCode::TOO_MANY_REQUESTS,
                request_id,
                timestamp,
                "Too many concurrent requests, try again later",
            ));
        }
    };

    tracing::info!(
        request_id = %request_id,
        message_len = request.message.len(),
        project = %request.project_path,
        "Sync endpoint request"
    );

    let (tx, mut rx) = mpsc::channel::<ChatEvent>(100);

    // Spawn the chat processing task
    tokio::spawn(async move {
        if let Err(e) = process_chat(state, request, tx.clone()).await {
            let _ = tx
                .send(ChatEvent::Error {
                    message: e.to_string(),
                })
                .await;
        }
        let _ = tx.send(ChatEvent::Done).await;
    });

    // Collect all events into a single response
    let mut content = String::new();
    let mut blocks: Vec<MessageBlock> = Vec::new();
    let mut usage: Option<UsageInfo> = None;
    let mut response_id: Option<String> = None;
    let mut previous_response_id: Option<String> = None;
    let mut error: Option<String> = None;

    while let Some(event) = rx.recv().await {
        match event {
            ChatEvent::TextDelta { delta } => {
                content.push_str(&delta);
            }
            ChatEvent::ToolCallStart { call_id, name, arguments, summary, category, .. } => {
                blocks.push(MessageBlock::ToolCall {
                    call_id,
                    name,
                    arguments,
                    summary,
                    category,
                    result: None,
                });
            }
            ChatEvent::ToolCallResult { call_id, name: _, success, output, duration_ms, truncated, total_bytes, diff, output_ref, exit_code, stderr } => {
                // Update the matching block with the result
                for block in &mut blocks {
                    if let MessageBlock::ToolCall { call_id: id, result, .. } = block {
                        if id == &call_id {
                            *result = Some(ToolCallResultData {
                                success,
                                output: output.clone(),
                                duration_ms,
                                truncated,
                                total_bytes,
                                diff: diff.clone(),
                                output_ref: output_ref.clone(),
                                exit_code,
                                stderr: stderr.clone(),
                            });
                            break;
                        }
                    }
                }
            }
            ChatEvent::Usage { input_tokens, output_tokens, reasoning_tokens, cached_tokens } => {
                // Accumulate usage across iterations
                usage = Some(match usage {
                    Some(u) => UsageInfo {
                        input_tokens: u.input_tokens + input_tokens,
                        output_tokens: u.output_tokens + output_tokens,
                        reasoning_tokens: u.reasoning_tokens + reasoning_tokens,
                        cached_tokens: u.cached_tokens + cached_tokens,
                    },
                    None => UsageInfo {
                        input_tokens,
                        output_tokens,
                        reasoning_tokens,
                        cached_tokens,
                    },
                });
            }
            ChatEvent::Chain { response_id: rid, previous_response_id: prev } => {
                response_id = rid;
                previous_response_id = prev;
            }
            ChatEvent::Error { message } => {
                error = Some(message);
            }
            ChatEvent::Done => break,
            ChatEvent::Reasoning { .. } => {} // Ignore reasoning summaries for sync
            ChatEvent::ReasoningDelta { .. } => {} // Ignore reasoning deltas for sync endpoint
            // New typed events - handle for sync endpoint
            ChatEvent::MessageStart { .. } | ChatEvent::MessageEnd { .. } => {} // Ignore boundaries
            ChatEvent::CodeBlockStart { id, language, filename } => {
                blocks.push(MessageBlock::CodeBlock {
                    language,
                    code: String::new(),
                    filename,
                });
                // Store ID for delta matching (hacky but works for now)
                content.push_str(&format!("\x00CB:{}", id));
            }
            ChatEvent::CodeBlockDelta { id, delta } => {
                // Find matching code block by ID marker
                let marker = format!("\x00CB:{}", id);
                if content.contains(&marker) {
                    // Append to last code block
                    if let Some(MessageBlock::CodeBlock { code, .. }) = blocks.last_mut() {
                        code.push_str(&delta);
                    }
                }
            }
            ChatEvent::CodeBlockEnd { id } => {
                // Remove the ID marker from content
                let marker = format!("\x00CB:{}", id);
                content = content.replace(&marker, "");
            }
            ChatEvent::Council { gpt, opus, gemini } => {
                blocks.push(MessageBlock::Council { gpt, opus, gemini });
            }
            ChatEvent::ArtifactCreated { .. } => {
                // Artifacts are tracked separately - ignore for sync response content
            }
            ChatEvent::Grounding { search_queries, sources } => {
                // Format grounding metadata as text for sync response
                let sources_text = sources.iter()
                    .map(|s| format!("- [{}]({})", s.title.as_deref().unwrap_or(&s.uri), s.uri))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !sources_text.is_empty() {
                    content.push_str(&format!("\n\n**Sources** ({})\n{}\n",
                        search_queries.join(", "),
                        sources_text
                    ));
                }
            }
            ChatEvent::CodeExecution { language, code, output, outcome } => {
                // Format code execution as a code block with result
                content.push_str(&format!("\n\n```{}\n{}\n```\n", language.to_lowercase(), code));
                if !output.is_empty() {
                    let status = if outcome == "OUTCOME_OK" { "Output" } else { "Error" };
                    content.push_str(&format!("\n**{}:**\n```\n{}\n```\n", status, output));
                }
            }
        }
    }

    // Prepend text content as first block if non-empty
    if !content.is_empty() {
        blocks.insert(0, MessageBlock::Text { content: content.clone() });
    }

    // Derive chain status: "NEW" if no previous, otherwise "…" + last 8 chars
    let chain = match &previous_response_id {
        None => "NEW".to_string(),
        Some(prev) => {
            let suffix = if prev.len() > 8 { &prev[prev.len() - 8..] } else { prev };
            format!("…{}", suffix)
        }
    };

    tracing::info!(
        request_id = %request_id,
        chain = %chain,
        input_tokens = usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
        output_tokens = usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
        "Sync endpoint complete"
    );

    Ok(Json(SyncChatResponse {
        request_id,
        timestamp,
        role: "assistant".to_string(),
        content,
        blocks,
        usage,
        response_id,
        previous_response_id,
        chain,
        success: error.is_none(),
        error,
    }))
}
