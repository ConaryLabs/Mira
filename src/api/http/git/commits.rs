// src/api/http/git/commits.rs
// MIGRATED: Updated to use unified ApiError and IntoApiError pattern
// Handlers for commit operations (history, diffs, file at commit)

use axum::{
    extract::{Path, State, Query},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

use crate::state::AppState;
use crate::api::error::{ApiResult, IntoApiError};
use serde::{Deserialize, Serialize};
use serde_json::json;

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
pub struct CommitHistoryResponse {
    pub commits: Vec<crate::git::types::CommitInfo>,
    pub total: usize,
    pub limit: usize,
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
/// MIGRATED: Now uses unified error handling pattern
pub async fn get_commit_history(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Query(params): Query<CommitHistoryQuery>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        // Get and validate attachment using unified error handling
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        // Get commit history with unified error handling
        let commits = state
            .git_client
            .get_commits(&attachment, params.limit)
            .into_api_error("Failed to retrieve commit history")?;
        
        let response = CommitHistoryResponse {
            total: commits.len(),
            limit: params.limit,
            commits,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

/// Get diff for a specific commit
/// MIGRATED: Now uses unified error handling pattern
pub async fn get_commit_diff(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, commit_hash)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        // Get and validate attachment using unified error handling
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        // Get commit diff with unified error handling
        let diff = state
            .git_client
            .get_diff(&attachment, &commit_hash)
            .into_api_error("Failed to retrieve commit diff")?;
        
        Ok(Json(diff))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

/// Get file content at a specific commit
/// MIGRATED: Now uses unified error handling pattern
pub async fn get_file_at_commit(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, commit_hash, file_path)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        // Get and validate attachment using unified error handling
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        // Get file content at commit with unified error handling
        let content = state
            .git_client
            .get_file_at_commit(&attachment, &commit_hash, &file_path)
            .into_api_error("Failed to retrieve file at commit")?;
        
        // Calculate actual file size
        let size = content.len();
        
        let response = FileContentResponse {
            path: file_path.clone(),
            content,
            language: super::common::detect_language(&file_path),
            encoding: "utf-8".to_string(),
            size,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}
