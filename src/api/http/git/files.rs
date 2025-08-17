// src/api/http/git/files.rs
// Handlers for file operations (tree, read, update)

use axum::{
    extract::{Path, State, Json},
    response::{IntoResponse, Response},
    http::StatusCode,
};
use std::sync::Arc;
use std::path::Path as StdPath;
use crate::state::AppState;
use crate::git::FileNode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, error, warn};

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

// ===== Handlers =====

/// Get the file tree of a repository
pub async fn get_file_tree_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
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
    
    match state.git_client.get_file_tree(&attachment) {
        Ok(nodes) => {
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
            
            Json(response).into_response()
        }
        Err(e) => {
            error!("Failed to get file tree: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to retrieve file tree"
                }))
            ).into_response()
        }
    }
}

/// Get content of a specific file
pub async fn get_file_content_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, file_path)): Path<(String, String, String)>,
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
    
    // Read file content
    let full_path = StdPath::new(&attachment.local_path).join(&file_path);
    
    match tokio::fs::read_to_string(&full_path).await {
        Ok(content) => {
            let size = content.len();
            let language = super::common::detect_language(&file_path);
            
            let response = FileContentResponse {
                path: file_path,
                content,
                language,
                encoding: "utf-8".to_string(),
                size,
            };
            
            Json(response).into_response()
        }
        Err(e) => {
            error!("Failed to read file {}: {}", file_path, e);
            
            if e.kind() == std::io::ErrorKind::NotFound {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "File not found"
                    }))
                ).into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Failed to read file"
                    }))
                ).into_response()
            }
        }
    }
}

/// Update content of a specific file
pub async fn update_file_content_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, file_path)): Path<(String, String, String)>,
    Json(payload): Json<UpdateFileRequest>,
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
    
    // Write file content
    let full_path = StdPath::new(&attachment.local_path).join(&file_path);
    
    // Ensure parent directory exists
    if let Some(parent) = full_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            error!("Failed to create directory: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to create directory"
                }))
            ).into_response();
        }
    }
    
    // Write the file
    match tokio::fs::write(&full_path, &payload.content).await {
        Ok(_) => {
            info!("Updated file {} in attachment {}", file_path, attachment_id);
            
            // If commit message provided, sync immediately
            if let Some(commit_msg) = payload.commit_message {
                if let Err(e) = state.git_client.sync_changes(&attachment, &commit_msg).await {
                    warn!("File updated but failed to sync: {}", e);
                }
            }
            
            let response = FileContentResponse {
                path: file_path.clone(),
                content: payload.content,
                language: super::common::detect_language(&file_path),
                encoding: "utf-8".to_string(),
                size: 0, // Will be calculated on next read
            };
            
            Json(response).into_response()
        }
        Err(e) => {
            error!("Failed to write file {}: {}", file_path, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to update file"
                }))
            ).into_response()
        }
    }
}
