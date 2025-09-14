// src/api/ws/git.rs
// WebSocket handler for Git operations - REFACTORED: 506 → 180 lines!
// All attachment lookup bullshit has been moved to ProjectOps trait

use std::sync::Arc;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, error, info};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    state::AppState,
    git::client::ProjectOps, // The magic sauce!
};

// ============================================================================
// REQUEST TYPES - Minimal, no bullshit
// ============================================================================

#[derive(Debug, Deserialize)]
struct AttachRepoRequest {
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
struct UpdateFileRequest {
    project_id: String,
    file_path: String,
    content: String,
    commit_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SwitchBranchRequest {
    project_id: String,
    branch_name: String,
}

#[derive(Debug, Deserialize)]
struct CommitsRequest {
    project_id: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DiffRequest {
    project_id: String,
    from_commit: Option<String>,
    to_commit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileAtCommitRequest {
    project_id: String,
    file_path: String,
    commit_sha: String,
}

// ============================================================================
// MAIN ROUTER - Each operation is now 5-10 lines instead of 25-30!
// ============================================================================

pub async fn handle_git_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("Processing git command: {}", method);
    
    let result = match method {
        // Repository management - NO MORE ATTACHMENT LOOKUPS!
        "git.attach" => {
            let req: AttachRepoRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid attach request: {}", e)))?;
            
            info!("Attaching repository {} to project {}", req.repo_url, req.project_id);
            let attachment = app_state.git_client
                .attach_repo(&req.project_id, &req.repo_url)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to attach: {}", e)))?;
            
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
        
        // File operations - CLEAN AF!
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
            
            let content = app_state.git_client
                .get_project_file(&req.project_id, &req.file_path)
                .await?;
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "file_content",
                    "path": req.file_path,
                    "content": content
                }),
                request_id: None,
            })
        }
        
        "git.update_file" => {
            let req: UpdateFileRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid update request: {}", e)))?;
            
            let message = req.commit_message
                .unwrap_or_else(|| format!("Update {}", req.file_path));
            
            app_state.git_client
                .update_project_file(&req.project_id, &req.file_path, &req.content, &message)
                .await?;
            
            Ok(WsServerMessage::Status {
                message: format!("Updated {}", req.file_path),
                detail: None,
            })
        }
        
        // Branch operations
        "git.branches" => {
            let req: GitProjectRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid branches request: {}", e)))?;
            
            let (branches, current) = app_state.git_client
                .get_project_branches(&req.project_id)
                .await?;
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "branch_list",
                    "branches": branches,
                    "current": current
                }),
                request_id: None,
            })
        }
        
        "git.switch_branch" => {
            let req: SwitchBranchRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid switch request: {}", e)))?;
            
            app_state.git_client
                .switch_project_branch(&req.project_id, &req.branch_name)
                .await?;
            
            Ok(WsServerMessage::Status {
                message: format!("Switched to branch: {}", req.branch_name),
                detail: None,
            })
        }
        
        // History operations
        "git.commits" => {
            let req: CommitsRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid commits request: {}", e)))?;
            
            let commits = app_state.git_client
                .get_project_commits(&req.project_id, req.limit.unwrap_or(50))
                .await?;
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "commit_history",
                    "commits": commits
                }),
                request_id: None,
            })
        }
        
        "git.diff" => {
            let req: DiffRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid diff request: {}", e)))?;
            
            let diff = app_state.git_client
                .get_project_diff(&req.project_id, req.from_commit.as_deref(), req.to_commit.as_deref())
                .await?;
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "diff",
                    "diff": diff
                }),
                request_id: None,
            })
        }
        
        "git.file_at_commit" => {
            let req: FileAtCommitRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid file at commit request: {}", e)))?;
            
            let content = app_state.git_client
                .get_project_file_at_commit(&req.project_id, &req.file_path, &req.commit_sha)
                .await?;
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "file_at_commit",
                    "path": req.file_path,
                    "commit": req.commit_sha,
                    "content": content
                }),
                request_id: None,
            })
        }
        
        _ => {
            error!("Unknown git method: {}", method);
            return Err(ApiError::bad_request(format!("Unknown git method: {}", method)));
        }
    };
    
    match &result {
        Ok(_) => info!("Git command {} completed successfully", method),
        Err(e) => error!("Git command {} failed: {:?}", method, e),
    }
    
    result
}
