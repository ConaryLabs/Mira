// src/api/ws/git.rs

use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::info;

use crate::api::error::ApiError;
use crate::api::ws::message::WsServerMessage;
use crate::state::AppState;
use crate::git::client::project_ops::ProjectOps;  // FIXED: Added trait import

#[derive(Debug, Deserialize)]
struct GitAttachRequest {
    project_id: String,
    repo_url: String,
}

#[derive(Debug, Deserialize)]
struct GitProjectRequest {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct SyncChangesRequest {
    project_id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct FileContentRequest {
    project_id: String,
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct RestoreFileRequest {
    project_id: String,
    file_path: String,
}

pub async fn handle_git_operation(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> Result<WsServerMessage> {
    match method {
        "git.attach" => {
            let req: GitAttachRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid attach request: {}", e)))?;
            
            info!("Attaching repo {} to project {}", req.repo_url, req.project_id);
            
            let attachment = app_state.git_client
                .attach_repo(&req.project_id, &req.repo_url)  // FIXED: attach_repo not attach_repository
                .await?;
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "repo_attached",
                    "attachment_id": attachment.id,
                    "repo_url": attachment.repo_url,
                    "local_path": attachment.local_path
                }),
                request_id: None,
            })
        }
        
        "git.clone" => {
            let req: GitProjectRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid clone request: {}", e)))?;
            
            let attachment = app_state.git_client.clone_project(&req.project_id).await?;
            
            Ok(WsServerMessage::Status {
                message: format!("Repository cloned to {}", attachment.local_path),
                detail: None,
            })
        }
        
        "git.import" => {
            let req: GitProjectRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid import request: {}", e)))?;
            
            app_state.git_client.import_project(&req.project_id).await?;
            
            Ok(WsServerMessage::Status {
                message: "Codebase imported successfully".to_string(),
                detail: None,
            })
        }
        
        "git.sync" => {
            let req: SyncChangesRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid sync request: {}", e)))?;
            
            info!("Syncing project {} with message: {}", req.project_id, req.message);
            app_state.git_client.sync_project(&req.project_id, &req.message).await?;
            
            Ok(WsServerMessage::Status {
                message: "Changes pushed to GitHub".to_string(),
                detail: None,
            })
        }
        
        "git.pull" => {
            let req: GitProjectRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid pull request: {}", e)))?;
            
            app_state.git_client.pull_project(&req.project_id).await?;
            
            Ok(WsServerMessage::Status {
                message: "Latest changes pulled from remote".to_string(),
                detail: None,
            })
        }
        
        "git.reset" => {
            let req: GitProjectRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid reset request: {}", e)))?;
            
            app_state.git_client.reset_project(&req.project_id).await?;
            
            Ok(WsServerMessage::Status {
                message: "Reset to remote HEAD".to_string(),
                detail: None,
            })
        }
        
        // Restore file from git (for undo functionality)
        "git.restore" => {
            let req: RestoreFileRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid restore request: {}", e)))?;
            
            info!("Restoring file {} in project {}", req.file_path, req.project_id);
            
            // Get project attachment to find repo path
            let attachment = app_state.git_client.store
                .get_attachment(&req.project_id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to get attachment: {}", e)))?;
            
            // FIXED: Handle the Option
            let attachment = attachment.ok_or_else(|| ApiError::not_found("Attachment not found"))?;
            
            // Use git2 to restore the file
            let repo_path = attachment.local_path.clone();
            let file_path = req.file_path.clone();
            
            tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
                use git2::{Repository, build::CheckoutBuilder};
                
                let repo = Repository::open(&repo_path)
                    .map_err(|e| ApiError::internal(format!("Failed to open repo: {}", e)))?;
                
                // Create checkout builder for single file
                let mut checkout = CheckoutBuilder::new();
                checkout.path(&file_path);
                checkout.force();
                
                // Checkout HEAD version of the file
                repo.checkout_head(Some(&mut checkout))
                    .map_err(|e| ApiError::internal(format!("Failed to restore file: {}", e)))?;
                
                Ok(())
            }).await
                .map_err(|e| ApiError::internal(format!("Task failed: {}", e)))??;
            
            Ok(WsServerMessage::Status {
                message: format!("Restored {} from git", req.file_path),
                detail: None,
            })
        }
        
        // File operations
        "git.tree" => {
            let req: GitProjectRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid tree request: {}", e)))?;
            
            let tree = app_state.git_client.get_project_tree(&req.project_id).await?;
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "file_tree",
                    "tree": tree
                }),
                request_id: None,
            })
        }
        
        "git.file" => {
            let req: FileContentRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid file request: {}", e)))?;
            
            // FIXED: Get attachment first, then use it with get_file_content
            let attachment = app_state.git_client.store
                .get_attachment(&req.project_id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to get attachment: {}", e)))?
                .ok_or_else(|| ApiError::not_found("Project attachment not found"))?;
            
            let content = app_state.git_client.get_file_content(
                &attachment,
                &req.file_path
            )?;  // FIXED: Not async, no .await needed
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "file_content",
                    "path": req.file_path,
                    "content": content
                }),
                request_id: None,
            })
        }
        
        _ => Err(ApiError::not_found(format!("Unknown git method: {}", method)).into())  // FIXED: not_found instead of method_not_found
    }
}
