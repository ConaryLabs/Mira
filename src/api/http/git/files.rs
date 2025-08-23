// src/api/http/git/files.rs
// MIGRATED: Updated to use unified ApiError and IntoApiError pattern
// Handlers for file operations (tree, read, update)

use axum::{
    extract::{Path, State, Json},
    response::IntoResponse,
};
use std::sync::Arc;
use std::path::Path as StdPath;

use crate::state::AppState;
use crate::git::FileNode;
use crate::api::error::{ApiResult, IntoApiError, ApiError};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ===== Request/Response DTOs =====

#[derive(Debug, Serialize)]
pub struct FileTreeResponse {
    pub nodes: Vec<FileNode>,
    pub total_files: usize,
    pub total_directories: usize,
}

#[derive(Debug, Serialize)]
pub struct FileContentResponse {
    pub path: String,
    pub content: String,
    pub language: Option<String>,
    pub encoding: String,
    pub size: usize,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFileRequest {
    pub content: String,
    pub commit_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateFileResponse {
    pub path: String,
    pub content: String,
    pub language: Option<String>,
    pub encoding: String,
    pub size: usize,
    pub synced: bool,
    pub message: String,
}

// ===== Handlers =====

/// Get the file tree of a repository
/// MIGRATED: Now uses unified error handling pattern
pub async fn get_file_tree_handler(
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
        
        // Get file tree with unified error handling
        let nodes = state
            .git_client
            .get_file_tree(&attachment)
            .into_api_error("Failed to retrieve file tree")?;
        
        // Calculate statistics
        let total_files = nodes.iter()
            .filter(|n| n.node_type == crate::git::FileNodeType::File)
            .count();
        let total_directories = nodes.iter()
            .filter(|n| n.node_type == crate::git::FileNodeType::Directory)
            .count();
        
        let response = FileTreeResponse {
            nodes,
            total_files,
            total_directories,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

/// Get content of a specific file
/// MIGRATED: Now uses unified error handling pattern
pub async fn get_file_content_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, file_path)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        // Get and validate attachment using unified error handling
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        // Construct full file path
        let full_path = StdPath::new(&attachment.local_path).join(&file_path);
        
        // Read file content with proper error handling
        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ApiError::not_found("File not found")
                } else {
                    ApiError::internal("Failed to read file content")
                }
            })?;
        
        let size = content.len();
        let language = super::common::detect_language(&file_path);
        
        let response = FileContentResponse {
            path: file_path,
            content,
            language,
            encoding: "utf-8".to_string(),
            size,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

/// Update content of a specific file
/// MIGRATED: Now uses unified error handling pattern
pub async fn update_file_content_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, file_path)): Path<(String, String, String)>,
    Json(payload): Json<UpdateFileRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        // Get and validate attachment using unified error handling
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        // Construct full file path
        let full_path = StdPath::new(&attachment.local_path).join(&file_path);
        
        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .into_api_error("Failed to create parent directory")?;
        }
        
        // Write file content
        tokio::fs::write(&full_path, &payload.content)
            .await
            .into_api_error("Failed to write file content")?;
        
        info!("Updated file {} in attachment {}", file_path, attachment_id);
        
        // Attempt to sync changes if commit message is provided
        let synced = if let Some(commit_msg) = payload.commit_message {
            match state.git_client.sync_changes(&attachment, &commit_msg).await {
                Ok(_) => {
                    info!("Successfully synced changes for file {}", file_path);
                    true
                }
                Err(e) => {
                    warn!("File updated but failed to sync: {}", e);
                    false
                }
            }
        } else {
            false
        };
        
        let size = payload.content.len();
        let message = if synced {
            "File updated and synced successfully".to_string()
        } else if payload.commit_message.is_some() {
            "File updated but sync failed".to_string()
        } else {
            "File updated successfully".to_string()
        };
        
        let response = UpdateFileResponse {
            path: file_path.clone(),
            content: payload.content,
            language: super::common::detect_language(&file_path),
            encoding: "utf-8".to_string(),
            size,
            synced,
            message,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}
