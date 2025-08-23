// src/api/http/router.rs
// HTTP router composition for REST API endpoints

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use crate::state::AppState;
use super::{
    handlers::{health_handler, project_details_handler},
    chat::{get_chat_history, rest_chat_handler},
    git::{
        // Repository management
        attach_repo_handler,
        list_attached_repos_handler,
        sync_repo_handler,
        // File operations
        get_file_tree_handler,
        get_file_content_handler,
        update_file_content_handler,
        // Branch operations
        list_branches,
        switch_branch,
        // Commit operations
        get_commit_history,
        get_commit_diff,
        get_file_at_commit,
    },
};

/// Main HTTP router for health and chat endpoints
/// This router is nested under /api in main.rs
pub fn http_router(app_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        // Health endpoint
        .route("/health", get(health_handler))
        
        // Chat endpoints (REST)
        .route("/chat/history", get(get_chat_history))
        .route("/chat", post(rest_chat_handler))
        
        // Project endpoints
        .route("/project/:project_id", get(project_details_handler))
        
        .with_state(app_state)
}

/// Git router for project operations
/// This router is nested under /projects/:project_id/git in project/mod.rs
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
