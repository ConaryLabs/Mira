// src/studio/chat.rs
// Chat streaming handler and Anthropic API integration

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    http::StatusCode,
    Json,
};
use futures::stream::Stream;
use std::{convert::Infallible, time::Duration};
use tokio_stream::StreamExt;
use tracing::{info, error};

use super::types::{StudioState, ChatRequest, AnthropicRequest, AnthropicStreamEvent};
use super::context::{build_tiered_context, get_recent_messages};

/// Messages to keep verbatim in context
const CONTEXT_WINDOW_SIZE: usize = 20;

/// Message count before generating a rolling summary
const ROLLING_SUMMARY_THRESHOLD: usize = 100;

/// Stream chat response and persist messages
pub async fn chat_stream_handler(
    State(state): State<StudioState>,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let api_key = state.anthropic_key.clone()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Anthropic API key not configured".to_string()))?;

    let model = req.model.unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());
    let now = chrono::Utc::now().timestamp();

    // Get or create conversation
    let conversation_id = match req.conversation_id {
        Some(id) => {
            // Verify it exists
            let exists: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM studio_conversations WHERE id = $1"
            )
            .bind(&id)
            .fetch_optional(state.db.as_ref())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            if exists.is_none() {
                return Err((StatusCode::NOT_FOUND, "Conversation not found".to_string()));
            }
            id
        }
        None => {
            // Create new conversation
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO studio_conversations (id, created_at, updated_at) VALUES ($1, $2, $2)"
            )
            .bind(&id)
            .bind(now)
            .execute(state.db.as_ref())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            id
        }
    };

    // Save user message
    let user_msg_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO studio_messages (id, conversation_id, role, content, created_at) VALUES ($1, $2, 'user', $3, $4)"
    )
    .bind(&user_msg_id)
    .bind(&conversation_id)
    .bind(&req.message)
    .bind(now)
    .execute(state.db.as_ref())
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Update conversation timestamp
    sqlx::query("UPDATE studio_conversations SET updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(&conversation_id)
        .execute(state.db.as_ref())
        .await
        .ok();

    // Auto-generate title from first message
    let _ = sqlx::query(
        "UPDATE studio_conversations SET title = $1 WHERE id = $2 AND title IS NULL"
    )
    .bind(req.message.chars().take(50).collect::<String>())
    .bind(&conversation_id)
    .execute(state.db.as_ref())
    .await;

    // Get message count for rolling summary check
    let (msg_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM studio_messages WHERE conversation_id = $1"
    )
    .bind(&conversation_id)
    .fetch_one(state.db.as_ref())
    .await
    .unwrap_or((0,));

    // Build tiered context
    let system_prompt = build_tiered_context(&state, &conversation_id).await;

    // Get last N messages for the request
    let recent_messages = get_recent_messages(&state.db, &conversation_id, CONTEXT_WINDOW_SIZE).await;

    let anthropic_req = AnthropicRequest {
        model: model.clone(),
        max_tokens: req.max_tokens,
        messages: recent_messages,
        stream: true,
        system: Some(system_prompt),
    };

    info!("Starting streaming chat with model: {} (conversation: {}, {} messages)", model, conversation_id, msg_count);

    let response = state.http_client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&anthropic_req)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("API request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Anthropic API error: {} - {}", status, body);
        return Err((StatusCode::BAD_GATEWAY, format!("API error: {}", body)));
    }

    // Stream the response and collect for saving
    let byte_stream = response.bytes_stream();
    let db = state.db.clone();
    let conv_id = conversation_id.clone();
    let should_summarize = msg_count > 0 && (msg_count as usize).is_multiple_of(ROLLING_SUMMARY_THRESHOLD);
    let summary_state = state.clone();

    let event_stream = async_stream::stream! {
        let mut buffer = String::new();
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut full_response = String::new();

        // Send conversation ID first
        yield Ok(Event::default().event("conversation").data(&conv_id));

        tokio::pin!(byte_stream);

        // Helper to process SSE data and yield text
        fn process_sse_data(data: &str, full_response: &mut String) -> Option<String> {
            if data == "[DONE]" {
                return None;
            }

            match serde_json::from_str::<AnthropicStreamEvent>(data) {
                Ok(event) => {
                    if event.event_type == "content_block_delta" {
                        if let Some(delta) = event.delta {
                            if let Some(text) = delta.text {
                                full_response.push_str(&text);
                                return Some(text);
                            }
                        }
                    }
                    None
                }
                Err(e) => {
                    tracing::error!("SSE JSON parse error: {} - data: {:?}", e, &data[..data.len().min(100)]);
                    None
                }
            }
        }

        // Process complete SSE events from buffer
        fn extract_events(buffer: &str) -> (Vec<String>, String) {
            let mut events = Vec::new();
            let mut remaining = buffer.to_string();

            loop {
                let boundary = remaining.find("\r\n\r\n")
                    .map(|pos| (pos, 4))
                    .or_else(|| remaining.find("\n\n").map(|pos| (pos, 2)));

                match boundary {
                    Some((pos, len)) => {
                        let event_data = remaining[..pos].to_string();
                        remaining = remaining[pos + len..].to_string();

                        let mut data_parts = Vec::new();
                        for line in event_data.lines() {
                            if let Some(rest) = line.strip_prefix("data:") {
                                let value = rest.strip_prefix(' ').unwrap_or(rest);
                                data_parts.push(value.to_string());
                            }
                        }

                        if !data_parts.is_empty() {
                            events.push(data_parts.join("\n"));
                        }
                    }
                    None => break,
                }
            }

            (events, remaining)
        }

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    byte_buffer.extend_from_slice(&bytes);

                    let valid_up_to = match std::str::from_utf8(&byte_buffer) {
                        Ok(s) => {
                            buffer.push_str(s);
                            byte_buffer.len()
                        }
                        Err(e) => {
                            let valid = e.valid_up_to();
                            if valid > 0 {
                                buffer.push_str(std::str::from_utf8(&byte_buffer[..valid]).unwrap());
                            }
                            valid
                        }
                    };

                    if valid_up_to < byte_buffer.len() {
                        byte_buffer = byte_buffer[valid_up_to..].to_vec();
                    } else {
                        byte_buffer.clear();
                    }

                    let (events, remaining) = extract_events(&buffer);
                    buffer = remaining;

                    for data in events {
                        if data == "[DONE]" {
                            yield Ok(Event::default().data("[DONE]"));
                            continue;
                        }

                        if let Some(text) = process_sse_data(&data, &mut full_response) {
                            yield Ok(Event::default().data(&text));
                        }
                    }
                }
                Err(e) => {
                    error!("Stream error: {}", e);
                    yield Ok(Event::default().data(format!("[ERROR] {}", e)));
                    break;
                }
            }
        }

        // Process any remaining buffer
        if !buffer.trim().is_empty() {
            let (events, _) = extract_events(&(buffer + "\n\n"));
            for data in events {
                if let Some(text) = process_sse_data(&data, &mut full_response) {
                    yield Ok(Event::default().data(&text));
                }
            }
        }

        // Save assistant response
        let assistant_msg_id = uuid::Uuid::new_v4().to_string();
        let save_time = chrono::Utc::now().timestamp();
        let _ = sqlx::query(
            "INSERT INTO studio_messages (id, conversation_id, role, content, created_at) VALUES ($1, $2, 'assistant', $3, $4)"
        )
        .bind(&assistant_msg_id)
        .bind(&conv_id)
        .bind(&full_response)
        .bind(save_time)
        .execute(db.as_ref())
        .await;

        // Generate rolling summary if threshold reached
        if should_summarize {
            tokio::spawn(async move {
                generate_rolling_summary(summary_state, conv_id).await;
            });
        }
    };

    Ok(Sse::new(event_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive")
    ))
}

/// Generate a rolling summary for a conversation
async fn generate_rolling_summary(state: StudioState, conversation_id: String) {
    let api_key = match &state.anthropic_key {
        Some(k) => k.clone(),
        None => return,
    };

    let messages = sqlx::query_as::<_, (String, String)>(r#"
        SELECT role, content FROM studio_messages
        WHERE conversation_id = $1
        ORDER BY created_at ASC
    "#)
    .bind(&conversation_id)
    .fetch_all(state.db.as_ref())
    .await
    .unwrap_or_default();

    if messages.len() <= CONTEXT_WINDOW_SIZE {
        return;
    }

    let to_summarize = &messages[..messages.len().saturating_sub(CONTEXT_WINDOW_SIZE)];

    let conversation = to_summarize
        .iter()
        .map(|(role, content)| format!("{}: {}", role, content))
        .collect::<Vec<_>>()
        .join("\n\n");

    let summary_prompt = format!(
        "Summarize this conversation concisely (2-3 paragraphs). Focus on:\n\
        - Main topics discussed\n\
        - Key decisions or conclusions\n\
        - Important context for continuing the conversation\n\n\
        Conversation:\n{}\n\n\
        Summary:",
        conversation
    );

    let request = serde_json::json!({
        "model": "claude-sonnet-4-5-20250929",
        "max_tokens": 500,
        "messages": [{"role": "user", "content": summary_prompt}]
    });

    let response = match state.http_client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to generate rolling summary: {}", e);
            return;
        }
    };

    if !response.status().is_success() {
        error!("Rolling summary API error: {}", response.status());
        return;
    }

    let body: serde_json::Value = match response.json().await {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to parse summary response: {}", e);
            return;
        }
    };

    let summary = body["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();

    if summary.is_empty() {
        return;
    }

    let now = chrono::Utc::now().timestamp();
    let id = uuid::Uuid::new_v4().to_string();

    if let Err(e) = sqlx::query(
        "INSERT INTO rolling_summaries (id, session_id, summary, message_count, created_at) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(&id)
    .bind(&conversation_id)
    .bind(&summary)
    .bind(messages.len() as i32)
    .bind(now)
    .execute(state.db.as_ref())
    .await
    {
        error!("Failed to store rolling summary: {}", e);
        return;
    }

    info!("Generated rolling summary for conversation {} ({} messages)", conversation_id, messages.len());
}
