// src/api/http/git/mod.rs

pub mod common;
// For now, re-export handlers from the parent git.rs if it exists
// Or create stub handlers here

use axum::{
    extract::{Path, State, Json},
    response::IntoResponse,
    http::StatusCode,
};
use std::sync::Arc;
use crate::state::AppState;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct AttachRepoRequest {
    pub repo_url: String,
}

#[derive(Serialize)]
pub struct RepoAttachment {
    pub id: String,
    pub repo_url: String,
    pub status: String,
}

// Stub handlers - implement these properly later
pub async fn attach_repo_handler(
    State(_state): State<Arc<AppState>>,
    Path(_project_id): Path<String>,
    Json(_payload): Json<AttachRepoRequest>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn list_attached_repos_handler(
    State(_state): State<Arc<AppState>>,
    Path(_project_id): Path<String>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn sync_repo_handler(
    State(_state): State<Arc<AppState>>,
    Path((_project_id, _attachment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn get_file_tree_handler(
    State(_state): State<Arc<AppState>>,
    Path((_project_id, _attachment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn get_file_content_handler(
    State(_state): State<Arc<AppState>>,
    Path((_project_id, _attachment_id, _file_path)): Path<(String, String, String)>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn update_file_content_handler(
    State(_state): State<Arc<AppState>>,
    Path((_project_id, _attachment_id, _file_path)): Path<(String, String, String)>,
    _body: String,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

// Phase 3 handlers
pub async fn list_branches(
    State(_state): State<Arc<AppState>>,
    Path((_project_id, _attachment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn switch_branch(
    State(_state): State<Arc<AppState>>,
    Path((_project_id, _attachment_id, _branch)): Path<(String, String, String)>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn get_commit_history(
    State(_state): State<Arc<AppState>>,
    Path((_project_id, _attachment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn get_commit_diff(
    State(_state): State<Arc<AppState>>,
    Path((_project_id, _attachment_id, _commit_hash)): Path<(String, String, String)>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn get_file_at_commit(
    State(_state): State<Arc<AppState>>,
    Path((_project_id, _attachment_id, _commit_hash, _file_path)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}
