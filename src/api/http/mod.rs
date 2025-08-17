// src/api/http/mod.rs
use axum::{Router, routing::{get, post}, extract::State, Json, http::StatusCode};
use std::sync::Arc;
use crate::state::AppState;
use serde::{Serialize, Deserialize};

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

pub use project::{
    project_details_handler,
};

// History response types
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
    30
}

// Handler for fetching Peter's eternal session history
pub async fn get_chat_history(
    State(app_state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<HistoryQuery>,
) -> Result<Json<ChatHistoryResponse>, StatusCode> {
    // Calculate how many messages to skip and take
    let skip = query.offset;
    let take = query.limit;
    
    // Fetch messages for peter-eternal session
    // Get more than requested to handle offset
    match app_state.memory_service
        .get_recent_context("peter-eternal", skip + take)
        .await 
    {
        Ok(memories) => {
            // Skip the offset amount and take the limit
            let messages: Vec<ChatHistoryMessage> = memories
                .into_iter()
                .skip(skip)
                .take(take)
                .enumerate()
                .map(|(idx, m)| ChatHistoryMessage {
                    id: format!("history-{}-{}", m.timestamp.timestamp(), idx),
                    role: if m.role == "assistant" { "assistant".to_string() } else { m.role },
                    content: m.content,
                    timestamp: m.timestamp.timestamp(),
                    tags: m.tags,
                })
                .collect();
            
            tracing::info!("📚 Loaded {} messages from Peter's eternal session (offset: {}, limit: {})", 
                         messages.len(), skip, take);
            
            Ok(Json(ChatHistoryResponse { messages }))
        }
        Err(e) => {
            tracing::error!("Failed to load Peter's history: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub fn http_router() -> Router<Arc<AppState>> {
    Router::new()
        // Chat history endpoint - matches frontend expectation
        .route("/chat/history", get(get_chat_history))
        
        // Git endpoints - existing
        .route(
            "/projects/:project_id/git/attach",
            post(attach_repo_handler),
        )
        .route(
            "/projects/:project_id/git/repos",
            get(list_attached_repos_handler),
        )
        .route(
            "/projects/:project_id/git/:attachment_id/sync",
            post(sync_repo_handler),
        )
        
        // Git file operations - existing
        .route(
            "/projects/:project_id/git/:attachment_id/tree",
            get(get_file_tree_handler),
        )
        .route(
            "/projects/:project_id/git/:attachment_id/file/*file_path",
            get(get_file_content_handler)
                .put(update_file_content_handler),
        )
        // Add the /files/* route that frontend expects (with 's')
        .route(
            "/projects/:project_id/git/:attachment_id/files/*file_path",
            get(get_file_content_handler)
                .put(update_file_content_handler),
        )
        
        // Git Phase 3 - new branch operations
        .route(
            "/projects/:project_id/git/:attachment_id/branches",
            get(list_branches),
        )
        .route(
            "/projects/:project_id/git/:attachment_id/branches/switch",
            post(switch_branch),
        )
        
        // Git Phase 3 - new commit operations
        .route(
            "/projects/:project_id/git/:attachment_id/commits",
            get(get_commit_history),
        )
        .route(
            "/projects/:project_id/git/:attachment_id/commits/:commit_id/diff",
            get(get_commit_diff),
        )
        .route(
            "/projects/:project_id/git/:attachment_id/file_at_commit",
            get(get_file_at_commit),
        )
        
        // Project endpoints
        .route(
            "/projects/:project_id/details",
            get(project_details_handler),
        )
        // Add other endpoints here as needed
}
