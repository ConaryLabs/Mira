// src/api/http/git/files.rs
// Complete migration to unified ApiError pattern

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
pub async fn get_file_tree_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        let nodes = state
            .git_client
            .get_file_tree(&attachment)
            .into_api_error("Failed to retrieve file tree")?;
        
        // Count files vs directories
        let (total_files, total_directories) = nodes.iter().fold((0, 0), |(files, dirs), node| {
            if node.node_type == crate::git::FileNodeType::File {
                (files + 1, dirs)
            } else {
                (files, dirs + 1)
            }
        });
        
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

/// Get file content from the repository
pub async fn get_file_content_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, file_path)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        let content = state
            .git_client
            .get_file_content(&attachment, &file_path)
            .into_api_error("Failed to read file content")?;
        
        let language = super::common::detect_language(&file_path);
        let size = content.len();
        
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

/// Update file content in the repository
pub async fn update_file_content_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, file_path)): Path<(String, String, String)>,
    Json(payload): Json<UpdateFileRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let attachment = super::common::get_validated_attachment(
            &state.git_client.store,
            &project_id,
            &attachment_id,
        ).await?;
        
        // Update file content
        state
            .git_client
            .update_file_content(
                &attachment,
                &file_path,
                &payload.content,
                payload.commit_message.as_deref(),
            )
            .into_api_error("Failed to update file content")?;
        
        info!("Updated file: {} ({} bytes)", file_path, payload.content.len());
        
        let language = super::common::detect_language(&file_path);
        let size = payload.content.len();
        
        let response = UpdateFileResponse {
            path: file_path,
            content: payload.content,
            language,
            encoding: "utf-8".to_string(),
            size,
            synced: true,
            message: "File updated successfully".to_string(),
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}
