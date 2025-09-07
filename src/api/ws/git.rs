// src/api/ws/git.rs
// WebSocket handler implementation for Git operations

use std::sync::Arc;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, error, info};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    git::GitRepoAttachment,
    state::AppState,
};

// Request types for Git operations

#[derive(Debug, Deserialize)]
struct AttachRepoRequest {
    project_id: String,
    repo_url: String,
}

#[derive(Debug, Deserialize)]
struct ListReposRequest {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct CloneRepoRequest {
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
struct ImportCodebaseRequest {
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
struct SyncRepoRequest {
    attachment_id: String,
    commit_message: String,
}

#[derive(Debug, Deserialize)]
struct ListBranchesRequest {
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
struct SwitchBranchRequest {
    attachment_id: String,
    branch_name: String,
}

#[derive(Debug, Deserialize)]
struct ListCommitsRequest {
    attachment_id: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct GetDiffRequest {
    attachment_id: String,
    commit_id: String,
}

#[derive(Debug, Deserialize)]
struct ListFilesRequest {
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
struct GetFileRequest {
    attachment_id: String,
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct UpdateFileRequest {
    attachment_id: String,
    file_path: String,
    content: String,
    commit_message: Option<String>,
}

/// Main entry point for all Git-related WebSocket commands
pub async fn handle_git_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("Processing git command: {} with params: {:?}", method, params);
    
    let result = match method {
        // Repository management
        "git.attach_repo" => attach_repository(params, app_state).await,
        "git.list_repos" => list_repositories(params, app_state).await,
        "git.clone_repo" => clone_repository(params, app_state).await,
        "git.import_codebase" => import_codebase(params, app_state).await,
        "git.sync_repo" => sync_repository(params, app_state).await,
        
        // Branch operations
        "git.list_branches" => list_branches(params, app_state).await,
        "git.switch_branch" => switch_branch(params, app_state).await,
        
        // Commit operations
        "git.list_commits" => list_commits(params, app_state).await,
        "git.get_diff" => get_commit_diff(params, app_state).await,
        
        // File operations
        "git.list_files" => list_files(params, app_state).await,
        "git.get_file" => get_file_content(params, app_state).await,
        "git.update_file" => update_file_content(params, app_state).await,
        
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

async fn attach_repository(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: AttachRepoRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid attach repo request: {e}")))?;
    
    info!("Attaching repository {} to project {}", request.repo_url, request.project_id);
    
    // Verify project exists
    let project = app_state.project_store
        .get_project(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to verify project: {e}")))?;
    
    if project.is_none() {
        return Err(ApiError::not_found(format!("Project not found: {}", request.project_id)));
    }
    
    // Attach the repository
    let attachment = app_state.git_client
        .attach_repo(&request.project_id, &request.repo_url)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to attach repository: {e}")))?;
    
    info!("Repository attached successfully: {}", attachment.id);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "repo_attached",
            "attachment": attachment_to_json(&attachment)
        }),
        request_id: None,
    })
}

async fn list_repositories(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: ListReposRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid list repos request: {e}")))?;
    
    debug!("Listing repositories for project: {}", request.project_id);
    
    let attachments = app_state.git_store
        .get_attachments_for_project(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list repositories: {e}")))?;
    
    info!("Found {} repositories for project {}", attachments.len(), request.project_id);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "repo_list",
            "project_id": request.project_id,
            "repositories": attachments.iter().map(attachment_to_json).collect::<Vec<_>>()
        }),
        request_id: None,
    })
}

async fn clone_repository(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: CloneRepoRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid clone repo request: {e}")))?;
    
    info!("Cloning repository: {}", request.attachment_id);
    
    // Get the attachment
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    
    // Clone the repository
    app_state.git_client
        .clone_repo(&attachment)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to clone repository: {e}")))?;
    
    info!("Repository cloned successfully: {}", attachment.id);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "repo_cloned",
            "attachment_id": attachment.id,
            "status": "cloned"
        }),
        request_id: None,
    })
}

async fn import_codebase(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: ImportCodebaseRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid import request: {e}")))?;
    
    info!("Importing codebase for repository: {}", request.attachment_id);
    
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    
    // Import the codebase
    app_state.git_client
        .import_codebase(&attachment)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to import codebase: {e}")))?;
    
    info!("Codebase imported successfully: {}", attachment.id);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "codebase_imported",
            "attachment_id": attachment.id,
            "status": "imported"
        }),
        request_id: None,
    })
}

async fn sync_repository(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: SyncRepoRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid sync request: {e}")))?;
    
    info!("Syncing repository: {}", request.attachment_id);
    
    if request.commit_message.trim().is_empty() {
        return Err(ApiError::bad_request("Commit message cannot be empty"));
    }
    
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    
    // Sync changes
    app_state.git_client
        .sync_changes(&attachment, &request.commit_message)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to sync repository: {e}")))?;
    
    info!("Repository synced successfully: {}", attachment.id);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "repo_synced",
            "attachment_id": attachment.id,
            "commit_message": request.commit_message
        }),
        request_id: None,
    })
}

// Branch Operations

async fn list_branches(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: ListBranchesRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid list branches request: {e}")))?;
    
    debug!("Listing branches for repository: {}", request.attachment_id);
    
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    
    let branches = app_state.git_client
        .get_branches(&attachment)
        .map_err(|e| ApiError::internal(format!("Failed to get branches: {e}")))?;
    
    info!("Found {} branches", branches.len());
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "branch_list",
            "attachment_id": attachment.id,
            "branches": branches
        }),
        request_id: None,
    })
}

async fn switch_branch(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: SwitchBranchRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid switch branch request: {e}")))?;
    
    info!("Switching to branch {} in repository {}", request.branch_name, request.attachment_id);
    
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    
    app_state.git_client
        .switch_branch(&attachment, &request.branch_name)
        .map_err(|e| ApiError::internal(format!("Failed to switch branch: {e}")))?;
    
    info!("Switched to branch: {}", request.branch_name);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "branch_switched",
            "attachment_id": attachment.id,
            "branch_name": request.branch_name
        }),
        request_id: None,
    })
}

// Commit Operations

async fn list_commits(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: ListCommitsRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid list commits request: {e}")))?;
    
    debug!("Listing commits for repository: {}", request.attachment_id);
    
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    let limit = request.limit.unwrap_or(50);
    
    let commits = app_state.git_client
        .get_commits(&attachment, limit)
        .map_err(|e| ApiError::internal(format!("Failed to get commits: {e}")))?;
    
    info!("Found {} commits", commits.len());
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "commit_list",
            "attachment_id": attachment.id,
            "commits": commits
        }),
        request_id: None,
    })
}

async fn get_commit_diff(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GetDiffRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid diff request: {e}")))?;
    
    debug!("Getting diff for commit {} in repository {}", request.commit_id, request.attachment_id);
    
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    
    let diff = app_state.git_client
        .get_diff(&attachment, &request.commit_id)
        .map_err(|e| ApiError::internal(format!("Failed to get diff: {e}")))?;
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "commit_diff",
            "attachment_id": attachment.id,
            "diff": diff
        }),
        request_id: None,
    })
}

// File Operations

async fn list_files(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: ListFilesRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid list files request: {e}")))?;
    
    debug!("Listing files for repository: {}", request.attachment_id);
    
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    
    let file_tree = app_state.git_client
        .get_file_tree(&attachment)
        .map_err(|e| ApiError::internal(format!("Failed to get file tree: {e}")))?;
    
    info!("Retrieved file tree with {} top-level items", file_tree.len());
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "file_list",
            "attachment_id": attachment.id,
            "files": file_tree
        }),
        request_id: None,
    })
}

async fn get_file_content(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GetFileRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid get file request: {e}")))?;
    
    debug!("Getting file {} from repository {}", request.file_path, request.attachment_id);
    
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    
    let content = app_state.git_client
        .get_file_content(&attachment, &request.file_path)
        .map_err(|e| ApiError::internal(format!("Failed to get file content: {e}")))?;
    
    // Detect language from file extension
    let language = detect_language(&request.file_path);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "file_content",
            "attachment_id": attachment.id,
            "file_path": request.file_path,
            "content": content,
            "language": language
        }),
        request_id: None,
    })
}

async fn update_file_content(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: UpdateFileRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid update file request: {e}")))?;
    
    info!("Updating file {} in repository {}", request.file_path, request.attachment_id);
    
    let attachment = get_validated_attachment(&request.attachment_id, &app_state).await?;
    
    app_state.git_client
        .update_file_content(
            &attachment,
            &request.file_path,
            &request.content,
            request.commit_message.as_deref()
        )
        .map_err(|e| ApiError::internal(format!("Failed to update file: {e}")))?;
    
    info!("File updated successfully: {}", request.file_path);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "file_updated",
            "attachment_id": attachment.id,
            "file_path": request.file_path
        }),
        request_id: None,
    })
}

// Helper Functions

/// Validates and retrieves a Git repository attachment
async fn get_validated_attachment(
    attachment_id: &str,
    app_state: &AppState,
) -> ApiResult<GitRepoAttachment> {
    app_state.git_store
        .get_attachment(attachment_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get attachment: {e}")))?
        .ok_or_else(|| ApiError::not_found(format!("Repository attachment not found: {attachment_id}")))
}

/// Converts attachment to JSON representation
fn attachment_to_json(attachment: &GitRepoAttachment) -> Value {
    json!({
        "id": attachment.id,
        "project_id": attachment.project_id,
        "repo_url": attachment.repo_url,
        "local_path": attachment.local_path,
        "import_status": attachment.import_status,
        "last_sync_at": attachment.last_sync_at.map(|dt| dt.to_rfc3339()),
        "last_imported_at": attachment.last_imported_at.map(|dt| dt.to_rfc3339())
    })
}

/// Detects programming language from file extension
fn detect_language(file_path: &str) -> Option<String> {
    let path = std::path::Path::new(file_path);
    let extension = path.extension()?.to_str()?;
    
    let language = match extension.to_lowercase().as_str() {
        "rs" => "rust",
        "js" | "mjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "c" => "c",
        "cpp" | "cc" | "cxx" => "cpp",
        "h" | "hpp" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "rb" => "ruby",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "sh" | "bash" => "bash",
        "sql" => "sql",
        "html" | "htm" => "html",
        "css" | "scss" | "sass" => "css",
        "json" => "json",
        "xml" => "xml",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "tex" => "latex",
        "r" => "r",
        "m" => "matlab",
        "jl" => "julia",
        "lua" => "lua",
        "vim" => "vim",
        "dockerfile" => "dockerfile",
        _ => return None,
    };
    
    Some(language.to_string())
}
