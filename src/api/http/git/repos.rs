// src/api/http/git/repos.rs
// Handlers for repository attachment, listing, and syncing

use axum::{
    extract::{Path, State, Json},
    response::{IntoResponse, Response},
    http::StatusCode,
};
use std::sync::Arc;
use crate::state::AppState;
use crate::git::GitRepoAttachment;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, error};

// ===== Request/Response DTOs =====

#[derive(Debug, Deserialize)]
pub struct AttachRepoRequest {
    pub repo_url: String,
}

#[derive(Debug, Serialize)]
pub struct AttachRepoResponse {
    pub id: String,
    pub project_id: String,
    pub repo_url: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct SyncRepoRequest {
    pub commit_message: String,
}

#[derive(Debug, Serialize)]
pub struct SyncRepoResponse {
    pub success: bool,
    pub message: String,
    pub last_sync_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListReposResponse {
    pub repos: Vec<GitRepoAttachment>,
    pub total: usize,
}

// ===== Handlers =====

/// Attach a new repository to a project
pub async fn attach_repo_handler(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
    Json(payload): Json<AttachRepoRequest>,
) -> Response {
    info!("Attaching repo {} to project {}", payload.repo_url, project_id);
    
    match state.git_client
        .attach_repo(&project_id, &payload.repo_url)
        .await {
        Ok(attachment) => {
            info!("Created attachment {} for repo {}", attachment.id, payload.repo_url);
            
            // Clone and import in background to avoid timeout
            let git_client = state.git_client.clone();
            let attachment_clone = attachment.clone();
            
            tokio::spawn(async move {
                info!("Starting clone and import for attachment {}", attachment_clone.id);
                
                if let Err(e) = git_client.clone_repo(&attachment_clone).await {
                    error!("Failed to clone repo: {}", e);
                    return;
                }
                
                info!("Successfully cloned repo to {}", attachment_clone.local_path);
                
                if let Err(e) = git_client.import_codebase(&attachment_clone).await {
                    error!("Failed to import codebase: {}", e);
                    return;
                }
                
                info!("Successfully imported codebase for attachment {}", attachment_clone.id);
            });
            
            let response = AttachRepoResponse {
                id: attachment.id,
                project_id: attachment.project_id,
                repo_url: attachment.repo_url,
                status: "cloning".to_string(),
                message: "Repository attached and cloning started".to_string(),
            };
            
            (StatusCode::CREATED, Json(response)).into_response()
        }
        Err(e) => {
            error!("Failed to attach repo: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("Failed to attach repository: {}", e)
                }))
            ).into_response()
        }
    }
}

/// List all repositories attached to a project
pub async fn list_attached_repos_handler(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> Response {
    match state.git_client.store.list_project_attachments(&project_id).await {
        Ok(repos) => {
            let response = ListReposResponse {
                total: repos.len(),
                repos,
            };
            Json(response).into_response()
        }
        Err(e) => {
            error!("Failed to list repos for project {}: {}", project_id, e);
            Json(json!({
                "error": "Failed to list repositories",
                "repos": []
            })).into_response()
        }
    }
}

/// Sync repository changes (commit and push)
pub async fn sync_repo_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Json(payload): Json<SyncRepoRequest>,
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
    
    info!("Syncing changes for attachment {}", attachment_id);
    
    match state.git_client.sync_changes(&attachment, &payload.commit_message).await {
        Ok(_) => {
            info!("Successfully synced changes for attachment {}", attachment_id);
            
            let response = SyncRepoResponse {
                success: true,
                message: format!("Changes committed and pushed with message: {}", payload.commit_message),
                last_sync_at: Some(chrono::Utc::now().to_rfc3339()),
            };
            
            Json(response).into_response()
        }
        Err(e) => {
            error!("Failed to sync changes: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("Failed to sync changes: {}", e)
                }))
            ).into_response()
        }
    }
}
