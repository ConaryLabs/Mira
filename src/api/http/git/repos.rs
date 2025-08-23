// src/api/http/git/repos.rs
// Complete migration to unified ApiError pattern

use axum::{
    extract::{Path, State, Json},
    response::IntoResponse,
    http::StatusCode,
};
use std::sync::Arc;

use crate::state::AppState;
use crate::git::GitRepoAttachment;
use crate::api::error::{ApiResult, IntoApiError};
use serde::{Deserialize, Serialize};
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
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        info!("Attaching repo {} to project {}", payload.repo_url, project_id);
        
        let attachment = state
            .git_client
            .attach_repo(&project_id, &payload.repo_url)
            .await
            .into_api_error("Failed to attach repository")?;
        
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
        
        Ok((StatusCode::CREATED, Json(response)))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

/// List all repositories attached to a project
pub async fn list_attached_repos_handler(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let repos = state
            .git_client
            .store
            .list_project_attachments(&project_id)
            .await
            .into_api_error("Failed to retrieve project repositories")?;
        
        let response = ListReposResponse {
            total: repos.len(),
            repos,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

/// Sync repository changes (commit and push)
pub async fn sync_repo_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Json(payload): Json<SyncRepoRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        info!("Syncing changes for attachment {}", attachment_id);
        
        state
            .git_client
            .sync_changes(&attachment, &payload.commit_message)
            .await
            .into_api_error("Failed to sync repository changes")?;
        
        info!("Successfully synced changes for attachment {}", attachment_id);
        
        let response = SyncRepoResponse {
            success: true,
            message: "Repository changes synced successfully".to_string(),
            last_sync_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}
