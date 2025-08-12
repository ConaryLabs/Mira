use axum::{
    extract::{Json, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;

use chrono::{TimeZone, Utc};

use crate::memory::types::MemoryEntry;
use crate::persona::PersonaOverlay;
use crate::services::chat::ChatService;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    /// Persona override is **not allowed** in this phase.
    /// If provided, we fail loudly so it's obvious something is mis‑wired.
    pub persona_override: Option<String>,
    /// Reserved for future retrieval (Phase 6)
    pub project_id: Option<String>,
    /// Reserved for future multimodal (Phase 5+)
    pub images: Option<Vec<String>>,
    pub pdfs: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ChatReply {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: f32,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

#[derive(Serialize)]
pub struct ChatHistoryResponse {
    pub messages: Vec<MemoryEntry>,
    pub session_id: String,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
    pub offset: Option<i64>,
    pub project_id: Option<String>,
}

/// Internal: shape we expect back when we request `response_format: { type: "json_object" }`.
/// This mirrors `ChatReply` so we can parse the model JSON and then map it 1:1.
#[derive(Deserialize)]
struct StructuredModelReply {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: f32,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

pub async fn chat_handler(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(payload): Json<ChatRequest>,
) -> Response {
    let session_id = "peter-eternal".to_string();
    eprintln!("Using eternal session: {}", session_id);

    // Persona policy: full-on or fail. No silent overrides.
    if let Some(ref override_str) = payload.persona_override {
        // We error loudly if the client tries to override persona at request time.
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(axum::body::Body::from(format!(
                "persona_override is not supported in this phase (got: {})",
                override_str
            )))
            .unwrap()
            .into_response();
    }

    // Call the unified GPT‑5 flow and request **strict JSON** so we can fill ChatReply.
    // NOTE: hybrid/project/images/pdfs are ignored in Phase 2; retrieval returns in Phase 6.
    let result = state
        .chat_service
        .process_message(&session_id, &payload.message, /* structured_json = */ true)
        .await;

    match result {
        Ok(res) => {
            // `res.text` SHOULD be JSON because we set structured_json=true.
            let parsed: StructuredModelReply = match serde_json::from_str(&res.text) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("❌ Failed to parse structured model JSON: {}", e);
                    // Return a minimal, safe response so the UI isn't bricked.
                    return Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(axum::body::Body::from("Model did not return valid JSON"))
                        .unwrap()
                        .into_response();
                }
            };

            let reply = ChatReply {
                output: parsed.output,
                persona: parsed.persona,
                mood: parsed.mood,
                salience: parsed.salience,
                summary: parsed.summary,
                memory_type: parsed.memory_type,
                tags: parsed.tags,
                intent: parsed.intent,
                monologue: parsed.monologue,
                reasoning_summary: parsed.reasoning_summary,
            };

            eprintln!("🎉 Chat handler complete, returning response");
            Json(reply).into_response()
        }
        Err(e) => {
            eprintln!("Chat service error: {:?}", e);
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(axum::body::Body::from("Internal model error"))
                .unwrap()
                .into_response()
        }
    }
}

pub async fn chat_history_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQuery>,
) -> impl IntoResponse {
    let session_id = "peter-eternal".to_string();
    let limit = params.limit.unwrap_or(30);
    let offset = params.offset.unwrap_or(0);

    eprintln!(
        "📜 Loading chat history: session={}, limit={}, offset={}",
        session_id, limit, offset
    );

    // Optional project filter (kept for API continuity; hybrid path returns later)
    let project_id_filter = params.project_id.clone();

    let query = if project_id_filter.is_some() {
        r#"
        SELECT id, session_id, role, content, timestamp, embedding, salience, tags,
               summary, memory_type, logprobs, moderation_flag, system_fingerprint
        FROM chat_history
        WHERE session_id = ? AND project_id = ?
        ORDER BY timestamp DESC
        LIMIT ? OFFSET ?
        "#
    } else {
        r#"
        SELECT id, session_id, role, content, timestamp, embedding, salience, tags,
               summary, memory_type, logprobs, moderation_flag, system_fingerprint
        FROM chat_history
        WHERE session_id = ?
        ORDER BY timestamp DESC
        LIMIT ? OFFSET ?
        "#
    };

    let rows = if let Some(ref project_id) = params.project_id {
        sqlx::query(query)
            .bind(&session_id)
            .bind(project_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&state.sqlite_store.pool)
            .await
    } else {
        sqlx::query(query)
            .bind(&session_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&state.sqlite_store.pool)
            .await
    };

    match rows {
        Ok(rows) => {
            eprintln!("✅ Found {} messages in history", rows.len());
            let mut messages = Vec::with_capacity(rows.len());

            for row in rows {
                let id: i64 = row.get("id");
                let session_id: String = row.get("session_id");
                let role: String = row.get("role");
                let content: String = row.get("content");
                let timestamp: chrono::NaiveDateTime = row.get("timestamp");
                let salience: Option<f32> = row.get("salience");
                let tags: Option<String> = row.get("tags");
                let summary: Option<String> = row.get("summary");
                let memory_type: Option<String> = row.get("memory_type");
                let moderation_flag: Option<bool> = row.get("moderation_flag");
                let system_fingerprint: Option<String> = row.get("system_fingerprint");

                let tags_vec = tags
                    .as_ref()
                    .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());

                let memory_type_enum = memory_type.as_ref().and_then(|mt| match mt.as_str() {
                    "Feeling" => Some(crate::memory::types::MemoryType::Feeling),
                    "Fact" => Some(crate::memory::types::MemoryType::Fact),
                    "Joke" => Some(crate::memory::types::MemoryType::Joke),
                    "Promise" => Some(crate::memory::types::MemoryType::Promise),
                    "Event" => Some(crate::memory::types::MemoryType::Event),
                    _ => Some(crate::memory::types::MemoryType::Other),
                });

                messages.push(MemoryEntry {
                    id: Some(id),
                    session_id,
                    role,
                    content,
                    timestamp: Utc.from_utc_datetime(&timestamp),
                    embedding: None,
                    salience,
                    tags: tags_vec,
                    summary,
                    memory_type: memory_type_enum,
                    logprobs: None,
                    moderation_flag,
                    system_fingerprint,
                });
            }

            let response = ChatHistoryResponse { messages, session_id };
            Json(response).into_response()
        }
        Err(e) => {
            eprintln!("❌ Failed to load chat history: {:?}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::from("Failed to load chat history"))
                .unwrap()
                .into_response()
        }
    }
}
