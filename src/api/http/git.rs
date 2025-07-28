use axum::{
    extract::{State, Path, Json},
    response::IntoResponse,
};
use std::sync::Arc;
use crate::handlers::AppState;
use crate::git::GitRepoAttachment;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct AttachRepoPayload {
    pub repo_url: String,
}

#[derive(Serialize)]
pub struct AttachRepoResponse {
    pub status: String,
    pub attachment: Option<GitRepoAttachment>,
    pub error: Option<String>,
}

pub async fn attach_repo_handler(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
    Json(payload): Json<AttachRepoPayload>,
) -> impl IntoResponse {
    let client = &state.git_client;
    match client.attach_repo(&project_id, &payload.repo_url).await {
        Ok(attachment) => {
            let client_clone = client.clone();
            let attachment_clone = attachment.clone();
            tokio::spawn(async move {
                let _ = client_clone.clone_repo(&attachment_clone).await;
                let _ = client_clone.import_codebase(&attachment_clone).await;
            });

            Json(AttachRepoResponse {
                status: "attached".to_string(),
                attachment: Some(attachment),
                error: None,
            })
        }
        Err(e) => Json(AttachRepoResponse {
            status: "error".to_string(),
            attachment: None,
            error: Some(e.to_string()),
        }),
    }
}

#[derive(Serialize)]
pub struct RepoListResponse {
    pub repos: Vec<GitRepoAttachment>,
}

pub async fn list_attached_repos_handler(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let store = &state.git_store;
    match store.get_attachments_for_project(&project_id).await {
        Ok(repos) => Json(RepoListResponse { repos }),
        Err(_) => Json(RepoListResponse { repos: Vec::new() }),
    }
}

#[derive(Deserialize)]
pub struct SyncRepoPayload {
    pub commit_message: String,
}

#[derive(Serialize)]
pub struct SyncRepoResponse {
    pub status: String,
    pub error: Option<String>,
}

// Full implementation without debug handler
pub async fn sync_repo_handler(
    State(app_state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Json(payload): Json<SyncRepoPayload>,
) -> Json<SyncRepoResponse> {
    let client = &app_state.git_client;
    let store = &app_state.git_store;
    
    // Get the attachment
    let attachment_result = store.get_attachment_by_id(&attachment_id).await;
    
    match attachment_result {
        Ok(Some(attachment)) => {
            // Check if attachment belongs to the project
            if attachment.project_id != project_id {
                return Json(SyncRepoResponse {
                    status: "not_found".to_string(),
                    error: Some("Attachment not found".to_string()),
                });
            }
            
            // Try to sync changes
            match client.sync_changes(&attachment, &payload.commit_message).await {
                Ok(_) => Json(SyncRepoResponse {
                    status: "synced".to_string(),
                    error: None,
                }),
                Err(e) => Json(SyncRepoResponse {
                    status: "error".to_string(),
                    error: Some(e.to_string()),
                }),
            }
        }
        Ok(None) => Json(SyncRepoResponse {
            status: "not_found".to_string(),
            error: Some("Attachment not found".to_string()),
        }),
        Err(e) => Json(SyncRepoResponse {
            status: "error".to_string(),
            error: Some(e.to_string()),
        }),
    }
}
