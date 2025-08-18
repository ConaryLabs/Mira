// src/api/http/mod.rs
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    extract::State,
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;

mod git;
mod project;

pub use git::{
    attach_repo_handler,
    list_attached_repos_handler,
    sync_repo_handler,
    get_file_tree_handler,
    get_file_content_handler,
    update_file_content_handler,
    // Phase 3 exports
    list_branches,
    switch_branch,
    get_commit_history,
    get_commit_diff,
    get_file_at_commit,
};

pub use project::project_details_handler;

// ---------- Health ----------

pub async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "model": "gpt-5",
        "timestamp": Utc::now().to_rfc3339()
    }))
}

// ---------- History response types ----------

#[derive(Serialize)]
pub struct ChatHistoryMessage {
    id: String,
    role: String,
    content: String,
    timestamp: i64,
    tags: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ChatHistoryResponse {
    messages: Vec<ChatHistoryMessage>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
}

fn default_limit() -> usize {
    std::env::var("MIRA_HISTORY_DEFAULT_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30)
}

// ---------- Chat history handler ----------

pub async fn get_chat_history(
    State(app_state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<HistoryQuery>,
) -> Result<Json<ChatHistoryResponse>, StatusCode> {
    // Get session ID from environment
    let session_id = std::env::var("MIRA_SESSION_ID")
        .unwrap_or_else(|_| "peter-eternal".to_string());

    tracing::info!(
        "📚 Fetching history for session: {} (offset: {}, limit: {})",
        session_id,
        query.offset,
        query.limit
    );

    // Enforce maximum limit from environment
    let max_limit = std::env::var("MIRA_HISTORY_MAX_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    // Calculate how many messages to skip and take
    let skip = query.offset;
    let take = query.limit.min(max_limit);

    // Fetch messages for session
    match app_state
        .memory_service
        .get_recent_context(&session_id, skip + take)
        .await
    {
        Ok(memories) => {
            tracing::info!("📚 Retrieved {} total memories from database", memories.len());

            let messages: Vec<ChatHistoryMessage> = memories
                .into_iter()
                .skip(skip)
                .take(take)
                .map(|m| ChatHistoryMessage {
                    id: format!("msg-{}-{}", m.timestamp.timestamp_millis(), m.id.unwrap_or(0)),
                    role: if m.role == "assistant" || m.role == "mira" {
                        "assistant".to_string()
                    } else {
                        m.role
                    },
                    content: m.content,
                    timestamp: m.timestamp.timestamp_millis(),
                    tags: m.tags,
                })
                .collect();

            tracing::info!("📚 Returning {} messages after pagination", messages.len());
            Ok(Json(ChatHistoryResponse { messages }))
        }
        Err(e) => {
            tracing::error!("❌ Failed to load history: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ---------- REST chat types & handler ----------

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub persona_override: Option<String>,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: usize,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: Option<String>,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

pub async fn rest_chat_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    // Get session ID from environment
    let session_id = std::env::var("MIRA_SESSION_ID")
        .unwrap_or_else(|_| "peter-eternal".to_string());

    // Use provided persona or default from environment
    let _persona_str = payload
        .persona_override
        .unwrap_or_else(|| {
            std::env::var("MIRA_DEFAULT_PERSONA").unwrap_or_else(|_| "default".to_string())
        });

    match app_state
        .chat_service
        .chat(&session_id, &payload.message, None)
        .await
    {
        Ok(response) => Ok(Json(ChatResponse {
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
        })),
        Err(e) => {
            tracing::error!("Chat service error: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ---------- Public router (UNPREFIXED). main.rs nests under /api ----------

pub fn http_router(app_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        // Health
        .route("/health", get(health_handler))
        // Chat endpoints (REST)
        .route("/chat/history", get(get_chat_history))
        .route("/chat", post(rest_chat_handler))
        // Git endpoints
        .route("/git/attach", post(attach_repo_handler))
        .route("/git/repos", get(list_attached_repos_handler))
        .route("/git/sync/:project_id", post(sync_repo_handler))
        .route("/git/tree/:project_id", get(get_file_tree_handler))
        .route("/git/file/:project_id", get(get_file_content_handler))
        .route("/git/file/:project_id", post(update_file_content_handler))
        .route("/git/branches/:project_id", get(list_branches))
        .route("/git/branch/:project_id", post(switch_branch))
        .route("/git/commits/:project_id", get(get_commit_history))
        .route("/git/diff/:project_id/:commit_sha", get(get_commit_diff))
        .route("/git/file-at-commit/:project_id/:commit_sha", get(get_file_at_commit))
        // Project endpoints
        .route("/project/:project_id", get(project_details_handler))
        .with_state(app_state)
}
