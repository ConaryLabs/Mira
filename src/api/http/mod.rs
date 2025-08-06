// src/api/http/mod.rs

use axum::{Router, routing::{get, post}};
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

pub use project::{
    project_details_handler,
};

pub fn http_router() -> Router<Arc<AppState>> {
    Router::new()
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
