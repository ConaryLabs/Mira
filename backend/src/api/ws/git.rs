// src/api/ws/git.rs

use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::info;

use crate::api::error::ApiError;
use crate::api::ws::message::WsServerMessage;
use crate::git::client::project_ops::ProjectOps;
use crate::state::AppState;

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

#[derive(Debug, Deserialize)]
struct GitDiffRequest {
    project_id: String,
    target: String,  // "uncommitted", "staged", "branch", "commit"
    base_branch: Option<String>,
    commit_hash: Option<String>,
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

            info!(
                "Attaching repo {} to project {}",
                req.repo_url, req.project_id
            );

            let attachment = app_state
                .git_client
                .attach_repo(&req.project_id, &req.repo_url)
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

            info!(
                "Syncing project {} with message: {}",
                req.project_id, req.message
            );
            app_state
                .git_client
                .sync_project(&req.project_id, &req.message)
                .await?;

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

            info!(
                "Restoring file {} in project {}",
                req.file_path, req.project_id
            );

            // Get attachments for project, not attachment by project_id
            let attachments = app_state
                .git_client
                .store
                .list_project_attachments(&req.project_id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;

            let attachment = attachments
                .first()
                .ok_or_else(|| ApiError::not_found("No repository attached to this project"))?;

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
            })
            .await
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

            let tree = app_state
                .git_client
                .get_project_tree(&req.project_id)
                .await?;

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

            // Get attachments for the project first
            let attachments = app_state
                .git_client
                .store
                .list_project_attachments(&req.project_id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;

            let attachment = attachments
                .first()
                .ok_or_else(|| ApiError::not_found("No repository attached to this project"))?;

            let content = app_state
                .git_client
                .get_file_content(&attachment, &req.file_path)?;

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "file_content",
                    "path": req.file_path,
                    "content": content
                }),
                request_id: None,
            })
        }

        "git.diff" => {
            let req: GitDiffRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid diff request: {}", e)))?;

            // Get attachments for the project first
            let attachments = app_state
                .git_client
                .store
                .list_project_attachments(&req.project_id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;

            let attachment = attachments
                .first()
                .ok_or_else(|| ApiError::not_found("No repository attached to this project"))?;

            let repo_path = attachment.local_path.clone();
            let target = req.target.clone();
            let base_branch = req.base_branch.clone();
            let commit_hash = req.commit_hash.clone();

            let diff = tokio::task::spawn_blocking(move || -> Result<String, ApiError> {
                use std::process::Command;

                let mut cmd = Command::new("git");
                cmd.current_dir(&repo_path);

                match target.as_str() {
                    "staged" => {
                        cmd.args(["diff", "--cached"]);
                    }
                    "branch" => {
                        let base = base_branch.unwrap_or_else(|| "main".to_string());
                        cmd.args(["diff", &format!("{}...HEAD", base)]);
                    }
                    "commit" => {
                        let hash = commit_hash.unwrap_or_else(|| "HEAD".to_string());
                        cmd.args(["show", &hash, "--format="]);
                    }
                    "uncommitted" | _ => {
                        cmd.args(["diff", "HEAD"]);
                    }
                }

                let output = cmd.output()
                    .map_err(|e| ApiError::internal(format!("Failed to run git diff: {}", e)))?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(ApiError::internal(format!("Git diff failed: {}", stderr)));
                }

                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            })
            .await
            .map_err(|e| ApiError::internal(format!("Task failed: {}", e)))??;

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "diff_result",
                    "diff": diff
                }),
                request_id: None,
            })
        }

        _ => Err(ApiError::not_found(format!("Unknown git method: {}", method)).into()),
    }
}
