// src/api/http/git/branches.rs
// MIGRATED: Updated to use unified ApiError and IntoApiError pattern
// Handlers for branch operations

use axum::{
    extract::{Path, State, Json},
    response::IntoResponse,
};
use std::sync::Arc;

use crate::state::AppState;
use crate::api::error::{ApiResult, IntoApiError};
use serde::{Deserialize, Serialize};
use tracing::info;

// ===== Request/Response DTOs =====

#[derive(Debug, Deserialize)]
pub struct SwitchBranchRequest {
    pub branch_name: String,
}

#[derive(Debug, Serialize)]
pub struct ListBranchesResponse {
    pub branches: Vec<crate::git::client::branch_manager::BranchInfo>,
    pub total: usize,
    pub current_branch: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SwitchBranchResponse {
    pub success: bool,
    pub message: String,
    pub current_branch: String,
    pub previous_branch: Option<String>,
}

// ===== Handlers =====

/// List all branches in the repository
/// MIGRATED: Now uses unified error handling pattern
pub async fn list_branches(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        // Get and validate attachment using unified error handling
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        // Get branches with unified error handling
        let branches = state
            .git_client
            .get_branches(&attachment)
            .into_api_error("Failed to retrieve branch list")?;
        
        // Find current branch
        let current_branch = branches
            .iter()
            .find(|b| b.is_head)
            .map(|b| b.name.clone());
        
        let response = ListBranchesResponse {
            total: branches.len(),
            current_branch,
            branches,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

/// Switch to a different branch
/// MIGRATED: Now uses unified error handling pattern
pub async fn switch_branch(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Json(payload): Json<SwitchBranchRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        // Get and validate attachment using unified error handling
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        // Get current branch for response info
        let previous_branch = state
            .git_client
            .get_branches(&attachment)
            .ok()
            .and_then(|branches| {
                branches
                    .iter()
                    .find(|b| b.is_head)
                    .map(|b| b.name.clone())
            });
        
        // Switch branch with unified error handling
        state
            .git_client
            .switch_branch(&attachment, &payload.branch_name)
            .into_api_error("Failed to switch branch")?;
        
        info!(
            "Switched to branch {} in attachment {} (from {})",
            payload.branch_name,
            attachment_id,
            previous_branch.as_deref().unwrap_or("unknown")
        );
        
        let response = SwitchBranchResponse {
            success: true,
            message: format!("Successfully switched to branch: {}", payload.branch_name),
            current_branch: payload.branch_name,
            previous_branch,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}
