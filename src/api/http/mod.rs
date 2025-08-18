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
                    id: m.id.map_or_else(
                        || "msg_unknown".to_string(),
                        |id| format!("msg_{}", id)
                    ),
                    role: m.role,
                    content: m.content,
                    timestamp: m.timestamp.timestamp(),
                    tags: m.tags.filter(|t| !t.is_empty()),
                })
                .collect();

            tracing::info!(
                "📚 Returning {} messages (skipped: {}, took: {})",
                messages.len(),
                skip,
                take
            );

            Ok(Json(ChatHistoryResponse { messages }))
        }
        Err(e) => {
            tracing::error!("❌ Error fetching chat history: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ---------- REST Chat Request/Response ----------

#[derive(Deserialize)]
pub struct RestChatRequest {
    pub message: String,
    pub project_id: Option<String>,
    pub persona_override: Option<String>,
    pub file_context: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct RestChatResponse {
    pub response: String,
    pub persona: String,
    pub mood: String,
    pub tags: Vec<String>,
    pub summary: String,
    pub tool_results: Option<Vec<serde_json::Value>>,
    pub citations: Option<Vec<serde_json::Value>>,
}

// ---------- REST chat handler ----------

pub async fn rest_chat_handler(
    State(app_state): State<Arc<AppState>>,
    Json(request): Json<RestChatRequest>,
) -> Result<Json<RestChatResponse>, StatusCode> {
    let session_id = std::env::var("MIRA_SESSION_ID")
        .unwrap_or_else(|_| "peter-eternal".to_string());

    tracing::info!("💬 REST chat request for session: {}", session_id);

    // Use regular chat method for now (chat_with_tools will be added in Phase 2)
    match app_state.chat_service.chat(
        &session_id,
        &request.message,
        request.project_id.as_deref(),
    ).await {
        Ok(response) => {
            Ok(Json(RestChatResponse {
                response: response.output,
                persona: response.persona,
                mood: response.mood,
                tags: response.tags,
                summary: response.summary,
                tool_results: None,  // Will be populated in Phase 2
                citations: None,     // Will be populated in Phase 2
            }))
        }
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
        // Project endpoints
        .route("/project/:project_id", get(project_details_handler))
        .with_state(app_state)
}

// ---------- Git router for projects (nested under /projects/:project_id/git) ----------

pub fn project_git_router() -> Router<Arc<AppState>> {
    Router::new()
        // Repository management
        .route("/attach", post(attach_repo_handler))
        .route("/repos", get(list_attached_repos_handler))
        .route("/sync/:attachment_id", post(sync_repo_handler))
        
        // File operations
        .route("/files/:attachment_id/tree", get(get_file_tree_handler))
        .route("/files/:attachment_id/content/*path", get(get_file_content_handler))
        .route("/files/:attachment_id/content/*path", post(update_file_content_handler))
        
        // Branch operations
        .route("/branches/:attachment_id", get(list_branches))
        .route("/branch/:attachment_id", post(switch_branch))
        
        // Commit operations
        .route("/commits/:attachment_id", get(get_commit_history))
        .route("/diff/:attachment_id/:commit_sha", get(get_commit_diff))
        .route("/file-at-commit/:attachment_id/:commit_sha/*path", get(get_file_at_commit))
}
