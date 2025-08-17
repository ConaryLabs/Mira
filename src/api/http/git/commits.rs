// src/api/http/git/commits.rs
// Handlers for commit operations (history, diffs, file at commit)

use axum::{
    extract::{Path, State, Query},
    response::{IntoResponse, Response},
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::error;

// ===== Request/Response DTOs =====

#[derive(Debug, Deserialize)]
pub struct CommitHistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Serialize)]
pub struct FileContentResponse {
    pub path: String,
    pub content: String,
    pub language: Option<String>,
    pub encoding: String,
    pub size: usize,
}

// ===== Handlers =====

/// Get commit history
pub async fn get_commit_history(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Query(params): Query<CommitHistoryQuery>,
) -> Response {
    // Get and validate attachment
    let attachment = match super::common::get_validated_attachment(
        &state.git_client.store, 
        &project_id, 
        &attachment_id
    ).await {
        Ok(att) => att,
        Err(response) => return response,
    };
    
    match state.git_client.get_commits(&attachment, params.limit) {
        Ok(commits) => Json(json!({
            "commits": commits,
            "total": commits.len(),
            "limit": params.limit,
        })).into_response(),
        Err(e) => {
            error!("Failed to get commit history: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to retrieve commit history"
                }))
            ).into_response()
        }
    }
}

/// Get diff for a specific commit
pub async fn get_commit_diff(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, commit_hash)): Path<(String, String, String)>,
) -> Response {
    // Get and validate attachment
    let attachment = match super::common::get_validated_attachment(
        &state.git_client.store, 
        &project_id, 
        &attachment_id
    ).await {
        Ok(att) => att,
        Err(response) => return response,
    };
    
    match state.git_client.get_diff(&attachment, &commit_hash) {
        Ok(diff) => Json(diff).into_response(),
        Err(e) => {
            error!("Failed to get diff for commit {}: {}", commit_hash, e);
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Failed to retrieve commit diff"
                }))
            ).into_response()
        }
    }
}

/// Get file content at a specific commit
pub async fn get_file_at_commit(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, commit_hash, file_path)): Path<(String, String, String, String)>,
) -> Response {
    // Get and validate attachment
    let attachment = match super::common::get_validated_attachment(
        &state.git_client.store, 
        &project_id, 
        &attachment_id
    ).await {
        Ok(att) => att,
        Err(response) => return response,
    };
    
    match state.git_client.get_file_at_commit(&attachment, &commit_hash, &file_path) {
        Ok(content) => {
            let response = FileContentResponse {
                path: file_path.clone(),
                content,
                language: super::common::detect_language(&file_path),
                encoding: "utf-8".to_string(),
                size: 0,
            };
            Json(response).into_response()
        }
        Err(e) => {
            error!("Failed to get file at commit: {}", e);
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Failed to retrieve file at commit"
                }))
            ).into_response()
        }
    }
}
