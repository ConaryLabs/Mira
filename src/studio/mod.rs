// src/studio/mod.rs
// Mira Studio API - Chat interface for Claude with memory integration

use axum::{
    Router,
    routing::{get, post},
    extract::{State, Path, Query},
    response::{
        sse::{Event, Sse},
        Json,
    },
    http::StatusCode,
};
use futures::stream::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio_stream::StreamExt;
use tracing::{info, error, debug};

use crate::tools::{SemanticSearch, memory, types::RecallRequest};

// === Constants ===

/// Messages to keep verbatim in context
const CONTEXT_WINDOW_SIZE: usize = 20;

/// Message count before generating a rolling summary
const ROLLING_SUMMARY_THRESHOLD: usize = 100;

// === Types ===

#[derive(Clone)]
pub struct StudioState {
    pub db: Arc<SqlitePool>,
    pub semantic: Arc<SemanticSearch>,
    pub http_client: Client,
    pub anthropic_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    /// Conversation ID (created if not provided)
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// The new user message
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_max_tokens() -> u32 {
    4096
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ConversationInfo {
    pub id: String,
    pub title: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
}

#[derive(Debug, Serialize)]
pub struct MessageInfo {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub before: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<ContentDelta>,
}

#[derive(Debug, Deserialize)]
struct ContentDelta {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    delta_type: Option<String>,
    text: Option<String>,
}

// === Router ===

pub fn router(state: StudioState) -> Router {
    Router::new()
        .route("/status", get(status_handler))
        .route("/conversations", get(list_conversations))
        .route("/conversations", post(create_conversation))
        .route("/conversations/{id}", get(get_conversation))
        .route("/conversations/{id}/messages", get(get_messages))
        .route("/chat/stream", post(chat_stream_handler))
        .with_state(state)
}

// === Handlers ===

async fn status_handler(
    State(state): State<StudioState>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "anthropic_configured": state.anthropic_key.is_some(),
        "semantic_search": state.semantic.is_available(),
    }))
}

/// List recent conversations
async fn list_conversations(
    State(state): State<StudioState>,
) -> Result<Json<Vec<ConversationInfo>>, (StatusCode, String)> {
    let conversations = sqlx::query_as::<_, (String, Option<String>, i64, i64)>(r#"
        SELECT c.id, c.title, c.created_at, c.updated_at
        FROM studio_conversations c
        ORDER BY c.updated_at DESC
        LIMIT 20
    "#)
    .fetch_all(state.db.as_ref())
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut result = Vec::new();
    for (id, title, created_at, updated_at) in conversations {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM studio_messages WHERE conversation_id = $1"
        )
        .bind(&id)
        .fetch_one(state.db.as_ref())
        .await
        .unwrap_or((0,));

        result.push(ConversationInfo {
            id,
            title,
            created_at,
            updated_at,
            message_count: count,
        });
    }

    Ok(Json(result))
}

/// Create a new conversation
async fn create_conversation(
    State(state): State<StudioState>,
) -> Result<Json<ConversationInfo>, (StatusCode, String)> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO studio_conversations (id, created_at, updated_at) VALUES ($1, $2, $2)"
    )
    .bind(&id)
    .bind(now)
    .execute(state.db.as_ref())
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ConversationInfo {
        id,
        title: None,
        created_at: now,
        updated_at: now,
        message_count: 0,
    }))
}

/// Get conversation info
async fn get_conversation(
    State(state): State<StudioState>,
    Path(id): Path<String>,
) -> Result<Json<ConversationInfo>, (StatusCode, String)> {
    let conv = sqlx::query_as::<_, (String, Option<String>, i64, i64)>(
        "SELECT id, title, created_at, updated_at FROM studio_conversations WHERE id = $1"
    )
    .bind(&id)
    .fetch_optional(state.db.as_ref())
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or((StatusCode::NOT_FOUND, "Conversation not found".to_string()))?;

    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM studio_messages WHERE conversation_id = $1"
    )
    .bind(&id)
    .fetch_one(state.db.as_ref())
    .await
    .unwrap_or((0,));

    Ok(Json(ConversationInfo {
        id: conv.0,
        title: conv.1,
        created_at: conv.2,
        updated_at: conv.3,
        message_count: count,
    }))
}

/// Get messages for a conversation (paginated)
async fn get_messages(
    State(state): State<StudioState>,
    Path(id): Path<String>,
    Query(query): Query<MessagesQuery>,
) -> Result<Json<Vec<MessageInfo>>, (StatusCode, String)> {
    let messages = if let Some(before_id) = query.before {
        // Get the created_at of the before message
        let before_time: Option<(i64,)> = sqlx::query_as(
            "SELECT created_at FROM studio_messages WHERE id = $1"
        )
        .bind(&before_id)
        .fetch_optional(state.db.as_ref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if let Some((before_time,)) = before_time {
            sqlx::query_as::<_, (String, String, String, i64)>(r#"
                SELECT id, role, content, created_at
                FROM studio_messages
                WHERE conversation_id = $1 AND created_at < $2
                ORDER BY created_at DESC
                LIMIT $3
            "#)
            .bind(&id)
            .bind(before_time)
            .bind(query.limit)
            .fetch_all(state.db.as_ref())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        } else {
            vec![]
        }
    } else {
        sqlx::query_as::<_, (String, String, String, i64)>(r#"
            SELECT id, role, content, created_at
            FROM studio_messages
            WHERE conversation_id = $1
            ORDER BY created_at DESC
            LIMIT $2
        "#)
        .bind(&id)
        .bind(query.limit)
        .fetch_all(state.db.as_ref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    // Reverse to get chronological order
    let result: Vec<MessageInfo> = messages.into_iter().rev().map(|(id, role, content, created_at)| {
        MessageInfo { id, role, content, created_at }
    }).collect();

    Ok(Json(result))
}

/// Stream chat response and persist messages
async fn chat_stream_handler(
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
    let should_summarize = msg_count > 0 && msg_count as usize % ROLLING_SUMMARY_THRESHOLD == 0;
    let summary_state = state.clone();

    let event_stream = async_stream::stream! {
        let mut buffer = String::new();
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut full_response = String::new();

        // Send conversation ID first
        yield Ok(Event::default().event("conversation").data(&conv_id));

        tokio::pin!(byte_stream);

        // Helper to process SSE data and yield text
        // Returns any text chunks to yield
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

        // Process complete SSE events from buffer, returning unconsumed portion
        fn extract_events(buffer: &str) -> (Vec<String>, String) {
            let mut events = Vec::new();
            let mut remaining = buffer.to_string();

            // SSE events are separated by blank lines (\n\n or \r\n\r\n)
            loop {
                // Look for event boundary - try both line ending styles
                let boundary = remaining.find("\r\n\r\n")
                    .map(|pos| (pos, 4))
                    .or_else(|| remaining.find("\n\n").map(|pos| (pos, 2)));

                match boundary {
                    Some((pos, len)) => {
                        let event_data = remaining[..pos].to_string();
                        remaining = remaining[pos + len..].to_string();

                        // Collect all data lines in this event (SSE spec allows multi-line data)
                        let mut data_parts = Vec::new();
                        for line in event_data.lines() {
                            // Handle both "data: value" and "data:value" (space is optional per SSE spec)
                            if let Some(rest) = line.strip_prefix("data:") {
                                let value = rest.strip_prefix(' ').unwrap_or(rest);
                                data_parts.push(value.to_string());
                            }
                            // Ignore event:, id:, retry:, and comments (:)
                        }

                        if !data_parts.is_empty() {
                            // SSE spec: join multiple data lines with newlines
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
                    // Append to byte buffer and decode only valid UTF-8
                    byte_buffer.extend_from_slice(&bytes);

                    // Find the last valid UTF-8 boundary
                    let valid_up_to = match std::str::from_utf8(&byte_buffer) {
                        Ok(s) => {
                            buffer.push_str(s);
                            byte_buffer.len()
                        }
                        Err(e) => {
                            // Decode up to the error point
                            let valid = e.valid_up_to();
                            if valid > 0 {
                                buffer.push_str(std::str::from_utf8(&byte_buffer[..valid]).unwrap());
                            }
                            valid
                        }
                    };

                    // Keep any incomplete UTF-8 sequence for next iteration
                    if valid_up_to < byte_buffer.len() {
                        byte_buffer = byte_buffer[valid_up_to..].to_vec();
                    } else {
                        byte_buffer.clear();
                    }

                    // Extract and process complete SSE events
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

        // Process any remaining buffer (in case stream ended without final blank line)
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
                generate_rolling_summary_for_conversation(summary_state, conv_id).await;
            });
        }
    };

    Ok(Sse::new(event_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive")
    ))
}

// === Helpers ===

/// Get recent messages from the database
async fn get_recent_messages(db: &SqlitePool, conversation_id: &str, limit: usize) -> Vec<ChatMessage> {
    let messages = sqlx::query_as::<_, (String, String)>(r#"
        SELECT role, content FROM studio_messages
        WHERE conversation_id = $1
        ORDER BY created_at DESC
        LIMIT $2
    "#)
    .bind(conversation_id)
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    // Reverse to get chronological order
    messages.into_iter().rev().map(|(role, content)| {
        ChatMessage { role, content }
    }).collect()
}

/// Build tiered context: persona + rolling summary + session context + semantic memories
async fn build_tiered_context(state: &StudioState, conversation_id: &str) -> String {
    let mut prompt_parts = Vec::new();

    // 1. Load persona
    let persona = load_persona(&state.db).await;
    prompt_parts.push(persona);

    // 2. Load rolling summary for this conversation (if exists)
    if let Some(rolling) = load_rolling_summary(&state.db, conversation_id).await {
        prompt_parts.push(format!(
            "\n<conversation_history>\nSummary of earlier parts of this conversation:\n{}\n</conversation_history>",
            rolling
        ));
    }

    // 3. Load recent session summaries from Claude Code sessions
    let sessions = load_recent_sessions(&state.db).await;
    if !sessions.is_empty() {
        prompt_parts.push(format!(
            "\n<session_context>\nRecent work sessions (what we've been working on):\n{}\n</session_context>",
            sessions
        ));
    }

    // 4. Recall semantic memories based on recent messages
    let recent = get_recent_messages(&state.db, conversation_id, 3).await;
    if !recent.is_empty() {
        let memories = recall_relevant_memories(state, &recent).await;
        if !memories.is_empty() {
            prompt_parts.push(format!(
                "\n<memories>\nRelevant details from memory:\n{}\n</memories>",
                memories
            ));
        }
    }

    prompt_parts.join("\n")
}

/// Load the conversation's rolling summary
async fn load_rolling_summary(db: &SqlitePool, conversation_id: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT summary FROM rolling_summaries WHERE session_id = $1 ORDER BY created_at DESC LIMIT 1"
    )
    .bind(conversation_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}

/// Generate a rolling summary for a conversation
async fn generate_rolling_summary_for_conversation(state: StudioState, conversation_id: String) {
    let api_key = match &state.anthropic_key {
        Some(k) => k.clone(),
        None => return,
    };

    // Get all messages beyond the context window
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

    // Summarize messages beyond the window
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

    // Store the rolling summary (using conversation_id as session_id)
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

/// Load recent session summaries for narrative context
async fn load_recent_sessions(db: &SqlitePool) -> String {
    let result = sqlx::query_as::<_, (String, String)>(r#"
        SELECT content, datetime(created_at, 'unixepoch', 'localtime') as created
        FROM memory_entries
        WHERE role = 'session_summary'
        ORDER BY created_at DESC
        LIMIT 3
    "#)
    .fetch_all(db)
    .await;

    match result {
        Ok(sessions) if !sessions.is_empty() => {
            info!("Loaded {} session summaries", sessions.len());
            sessions
                .into_iter()
                .map(|(content, created)| format!("[{}]\n{}", created, content))
                .collect::<Vec<_>>()
                .join("\n\n---\n\n")
        }
        _ => {
            debug!("No session summaries found");
            String::new()
        }
    }
}

/// Load persona from coding_guidelines
async fn load_persona(db: &SqlitePool) -> String {
    let result = sqlx::query_scalar::<_, String>(
        "SELECT content FROM coding_guidelines WHERE category = 'persona' ORDER BY priority DESC LIMIT 1"
    )
    .fetch_optional(db)
    .await;

    match result {
        Ok(Some(persona)) => persona,
        _ => default_persona(),
    }
}

/// Recall memories relevant to the conversation
async fn recall_relevant_memories(state: &StudioState, messages: &[ChatMessage]) -> String {
    let user_messages: Vec<&str> = messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .rev()
        .take(3)
        .collect();

    if user_messages.is_empty() {
        return String::new();
    }

    let query = user_messages.join(" ");
    debug!("Recalling memories for query: {}", &query[..query.len().min(100)]);

    let recall_req = RecallRequest {
        query,
        fact_type: None,
        category: None,
        limit: Some(5),
    };

    match memory::recall(&state.db, &state.semantic, recall_req, None).await {
        Ok(results) if !results.is_empty() => {
            let memory_lines: Vec<String> = results
                .iter()
                .filter_map(|r| {
                    let value = r.get("value").and_then(|v| v.as_str())?;
                    let fact_type = r.get("fact_type").and_then(|v| v.as_str()).unwrap_or("memory");
                    Some(format!("- [{}] {}", fact_type, value))
                })
                .collect();

            if memory_lines.is_empty() {
                String::new()
            } else {
                info!("Injected {} memories into context", memory_lines.len());
                memory_lines.join("\n")
            }
        }
        Ok(_) => {
            debug!("No relevant memories found");
            String::new()
        }
        Err(e) => {
            error!("Failed to recall memories: {}", e);
            String::new()
        }
    }
}

fn default_persona() -> String {
    r#"You are Mira, a friendly and helpful AI assistant. You have a warm, conversational personality while being knowledgeable and precise. You remember context from previous conversations and help users with coding, questions, and creative tasks.

Key traits:
- Warm and personable, but not overly formal
- Direct and helpful without being terse
- You can engage in casual conversation as well as technical work
- You have access to the user's project context and memories through Mira

When you recall memories from previous conversations, use them naturally in your responses without explicitly saying "I remember that..." unless it's contextually appropriate."#.to_string()
}
