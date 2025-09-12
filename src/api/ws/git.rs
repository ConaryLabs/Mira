// src/api/ws/git.rs
// WebSocket handler for all Git repository operations

use std::sync::Arc;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, error, info, warn};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    state::AppState,
};

// Request types for git operations

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
    commit_id: String,
}

#[derive(Debug, Deserialize)]
struct FileAtCommitRequest {
    project_id: String,
    commit_id: String,
    file_path: String,
}

/// Main entry point for all git-related WebSocket commands
pub async fn handle_git_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("Processing git command: {} with params: {:?}", method, params);
    
    let result = match method {
        // Repository management
        "git.attach" => attach_repo(params, app_state).await,
        "git.clone" => clone_repo(params, app_state).await,
        "git.import" => import_codebase(params, app_state).await,
        "git.sync" => sync_changes(params, app_state).await,
        "git.pull" => pull_changes(params, app_state).await,
        "git.reset" => reset_to_remote(params, app_state).await,
        
        // File operations
        "git.tree" => get_file_tree(params, app_state).await,
        "git.file" => get_file_content(params, app_state).await,
        "git.update_file" => update_file_content(params, app_state).await,
        
        // Branch operations
        "git.branches" => get_branches(params, app_state).await,
        "git.switch_branch" => switch_branch(params, app_state).await,
        
        // History operations
        "git.commits" => get_commits(params, app_state).await,
        "git.diff" => get_diff(params, app_state).await,
        "git.file_at_commit" => get_file_at_commit(params, app_state).await,
        
        _ => {
            error!("Unknown git method: {}", method);
            return Err(ApiError::bad_request(format!("Unknown git method: {method}")));
        }
    };
    
    match &result {
        Ok(_) => info!("Successfully processed git command: {}", method),
        Err(e) => error!("Failed to process git command {}: {:?}", method, e),
    }
    
    result
}

// Repository Management Operations

async fn attach_repo(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: AttachRepoRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid attach request: {e}")))?;
    
    info!("Attaching repository {} to project {}", request.repo_url, request.project_id);
    
    let attachment = app_state.git_client
        .attach_repo(&request.project_id, &request.repo_url)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to attach repository: {e}")))?;
    
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

async fn clone_repo(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GitProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid clone request: {e}")))?;
    
    info!("Cloning repository for project {}", request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        app_state.git_client
            .clone_repo(attachment)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to clone repository: {e}")))?;
        
        Ok(WsServerMessage::Status {
            message: format!("Repository cloned to {}", attachment.local_path),
            detail: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

async fn import_codebase(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GitProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid import request: {e}")))?;
    
    info!("Importing codebase for project {}", request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        app_state.git_client
            .import_codebase(attachment)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to import codebase: {e}")))?;
        
        Ok(WsServerMessage::Status {
            message: "Codebase imported successfully".to_string(),
            detail: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

async fn sync_changes(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: SyncChangesRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid sync request: {e}")))?;
    
    info!("Syncing changes for project {} with message: {}", request.project_id, request.message);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        app_state.git_client
            .sync_changes(attachment, &request.message)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to sync changes: {e}")))?;
        
        Ok(WsServerMessage::Status {
            message: "Changes pushed to GitHub".to_string(),
            detail: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

async fn pull_changes(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GitProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid pull request: {e}")))?;
    
    info!("Pulling changes for project {}", request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        app_state.git_client
            .pull_changes(&attachment.id)
            .await?;
        
        Ok(WsServerMessage::Status {
            message: "Successfully pulled changes from remote".to_string(),
            detail: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

async fn reset_to_remote(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GitProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid reset request: {e}")))?;
    
    warn!("Resetting repository for project {} - this will lose all local changes!", request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        app_state.git_client
            .reset_to_remote(&attachment.id)
            .await?;
        
        Ok(WsServerMessage::Status {
            message: "Repository reset to remote HEAD - all local changes lost".to_string(),
            detail: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

// File Operations

async fn get_file_tree(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GitProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid file tree request: {e}")))?;
    
    debug!("Getting file tree for project {}", request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        let tree = app_state.git_client
            .get_file_tree(attachment)
            .map_err(|e| ApiError::internal(format!("Failed to get file tree: {e}")))?;
        
        Ok(WsServerMessage::Data {
            data: json!({
                "type": "file_tree",
                "tree": tree
            }),
            request_id: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

async fn get_file_content(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: FileContentRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid file content request: {e}")))?;
    
    debug!("Getting file {} for project {}", request.file_path, request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        let content = app_state.git_client
            .get_file_content(attachment, &request.file_path)
            .map_err(|e| ApiError::internal(format!("Failed to get file content: {e}")))?;
        
        Ok(WsServerMessage::Data {
            data: json!({
                "type": "file_content",
                "path": request.file_path,
                "content": content
            }),
            request_id: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

async fn update_file_content(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: UpdateFileRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid update file request: {e}")))?;
    
    info!("Updating file {} for project {}", request.file_path, request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        let commit_message = request.commit_message
            .unwrap_or_else(|| format!("Update {}", request.file_path));
        
        app_state.git_client
            .update_file_content(
                attachment,
                &request.file_path,
                &request.content,
                Some(&commit_message),
            )
            .map_err(|e| ApiError::internal(format!("Failed to update file: {e}")))?;
        
        Ok(WsServerMessage::Status {
            message: format!("File {} updated and committed", request.file_path),
            detail: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

// Branch Operations

async fn get_branches(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GitProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid branches request: {e}")))?;
    
    debug!("Getting branches for project {}", request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        let branches = app_state.git_client
            .get_branches(attachment)
            .map_err(|e| ApiError::internal(format!("Failed to get branches: {e}")))?;
        
        Ok(WsServerMessage::Data {
            data: json!({
                "type": "branches",
                "branches": branches
            }),
            request_id: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

async fn switch_branch(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: SwitchBranchRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid switch branch request: {e}")))?;
    
    info!("Switching to branch {} for project {}", request.branch_name, request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        app_state.git_client
            .switch_branch(attachment, &request.branch_name)
            .map_err(|e| ApiError::internal(format!("Failed to switch branch: {e}")))?;
        
        Ok(WsServerMessage::Status {
            message: format!("Switched to branch {}", request.branch_name),
            detail: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

// History Operations

async fn get_commits(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: CommitsRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid commits request: {e}")))?;
    
    debug!("Getting commits for project {}", request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        let commits = app_state.git_client
            .get_commits(attachment, request.limit.unwrap_or(20))
            .map_err(|e| ApiError::internal(format!("Failed to get commits: {e}")))?;
        
        Ok(WsServerMessage::Data {
            data: json!({
                "type": "commits",
                "commits": commits
            }),
            request_id: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

async fn get_diff(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: DiffRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid diff request: {e}")))?;
    
    debug!("Getting diff for commit {} in project {}", request.commit_id, request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        let diff = app_state.git_client
            .get_diff(attachment, &request.commit_id)
            .map_err(|e| ApiError::internal(format!("Failed to get diff: {e}")))?;
        
        Ok(WsServerMessage::Data {
            data: json!({
                "type": "diff",
                "diff": diff
            }),
            request_id: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}

async fn get_file_at_commit(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: FileAtCommitRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid file at commit request: {e}")))?;
    
    debug!("Getting file {} at commit {} for project {}", 
           request.file_path, request.commit_id, request.project_id);
    
    let attachments = app_state.git_client.store
        .list_project_attachments(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {e}")))?;
    
    if let Some(attachment) = attachments.first() {
        let content = app_state.git_client
            .get_file_at_commit(attachment, &request.commit_id, &request.file_path)
            .map_err(|e| ApiError::internal(format!("Failed to get file at commit: {e}")))?;
        
        Ok(WsServerMessage::Data {
            data: json!({
                "type": "file_at_commit",
                "path": request.file_path,
                "commit_id": request.commit_id,
                "content": content
            }),
            request_id: None,
        })
    } else {
        Err(ApiError::not_found("No repository attached to this project"))
    }
}
