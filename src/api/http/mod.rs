// src/api/http/mod.rs

use axum::{Router, routing::{get, post}};
use std::sync::Arc;
use crate::handlers::AppState;

mod git;
mod project;

pub use git::{
    attach_repo_handler,
    list_attached_repos_handler,
    sync_repo_handler,
};
pub use project::{
    project_details_handler,
};

pub fn http_router() -> Router<Arc<AppState>> {
    Router::new()
        // Git endpoints
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
        .route(
            "/projects/:project_id/details",
            get(project_details_handler),
        )
        // Add other endpoints here as needed
}
