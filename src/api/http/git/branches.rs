// src/api/http/git/branches.rs
// Handlers for branch operations

use axum::{
    extract::{Path, State, Json},
    response::{IntoResponse, Response},
    http::StatusCode,
};
use std::sync::Arc;
use crate::state::AppState;
use serde::Deserialize;
use serde_json::json;
use tracing::{info, error};

// ===== Request/Response DTOs =====

#[derive(Debug, Deserialize)]
pub struct SwitchBranchRequest {
    pub branch_name: String,
}

// ===== Handlers =====

/// List all branches in the repository
pub async fn list_branches(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
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
    
    match state.git_client.get_branches(&attachment) {
        Ok(branches) => Json(json!({
            "branches": branches,
            "total": branches.len(),
        })).into_response(),
        Err(e) => {
            error!("Failed to list branches: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to list branches"
                }))
            ).into_response()
        }
    }
}

/// Switch to a different branch
pub async fn switch_branch(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Json(payload): Json<SwitchBranchRequest>,
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
    
    match state.git_client.switch_branch(&attachment, &payload.branch_name) {
        Ok(_) => {
            info!("Switched to branch {} in attachment {}", payload.branch_name, attachment_id);
            Json(json!({
                "success": true,
                "message": format!("Switched to branch: {}", payload.branch_name),
                "current_branch": payload.branch_name,
            })).into_response()
        }
        Err(e) => {
            error!("Failed to switch branch: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Failed to switch branch: {}", e)
                }))
            ).into_response()
        }
    }
}
