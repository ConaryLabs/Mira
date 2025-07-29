// src/handlers.rs

use axum::{
    extract::{Json, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::types::MemoryEntry;
use crate::llm::OpenAIClient;
use crate::persona::PersonaOverlay;
use crate::project::store::ProjectStore;
use crate::services::{ChatService, MemoryService, ContextService};
use chrono::{Utc, TimeZone};
use sqlx::Row;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub persona_override: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Serialize)]
pub struct ChatReply {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: u8,
    pub summary: Option<String>,
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

#[derive(Clone)]
pub struct AppState {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_store: Arc<QdrantMemoryStore>,
    pub llm_client: Arc<OpenAIClient>,
    pub project_store: Arc<ProjectStore>,
    pub git_store: crate::git::GitStore,
    pub git_client: crate::git::GitClient,
    
    // Add services
    pub chat_service: Arc<ChatService>,
    pub memory_service: Arc<MemoryService>,
    pub context_service: Arc<ContextService>,
}

pub async fn chat_handler(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(payload): Json<ChatRequest>,
) -> Response {
    let session_id = "peter-eternal".to_string();
    eprintln!("Using eternal session: {}", session_id);
    
    // Parse persona
    let persona = payload.persona_override
        .as_deref()
        .and_then(|s| s.parse::<PersonaOverlay>().ok())
        .unwrap_or(PersonaOverlay::Default);
    
    // Call service
    match state.chat_service
        .process_message(
            &session_id,
            &payload.message,
            &persona,
            payload.project_id.as_deref(),
        )
        .await
    {
        Ok(response) => {
            // Convert service response to API response
            let reply = ChatReply {
                output: response.output,
                persona: response.persona,
                mood: response.mood,
                salience: response.salience,
                summary: response.summary,
                memory_type: response.memory_type,
                tags: response.tags,
                intent: response.intent,
                monologue: response.monologue,
                reasoning_summary: response.reasoning_summary,
            };
            
            eprintln!("🎉 Chat handler complete, returning response");
            Json(reply).into_response()
        }
        Err(e) => {
            eprintln!("Chat service error: {:?}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::from("Internal server error"))
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

    eprintln!("📜 Loading chat history: session={}, limit={}, offset={}", 
        session_id, limit, offset
    );

    // Clone project_id to avoid move issues
    let project_id_filter = params.project_id.clone();

    // Build query with optional project filter
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
            let mut messages = Vec::new();
            
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

                // Deserialize tags
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
                    embedding: None, // Skip embedding for API response
                    salience,
                    tags: tags_vec,
                    summary,
                    memory_type: memory_type_enum,
                    logprobs: None,
                    moderation_flag,
                    system_fingerprint,
                });
            }
            
            // DON'T reverse - keep them in DESC order (newest first) for the frontend
            let response = ChatHistoryResponse {
                messages,
                session_id,
            };
            
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
