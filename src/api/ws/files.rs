// src/api/ws/files.rs
// Handles file upload and download operations through WebSocket
// Supports chunked transfers for large files (up to 500MB)

use std::sync::Arc;
use serde::Deserialize;
use serde_json::Value;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use uuid::Uuid;
use tracing::{debug, error, info};

use crate::state::{AppState, UploadSession};
use crate::api::ws::message::WsServerMessage;
use crate::api::error::{ApiError, ApiResult};

#[derive(Deserialize)]
struct UploadStartRequest {
    filename: String,
    content_type: String,
    total_size: usize,
    project_id: Option<String>,
}

#[derive(Deserialize)]
struct UploadChunkRequest {
    session_id: String,
    chunk: String,  // base64 encoded
    chunk_index: usize,
    is_final: bool,
}

#[derive(Deserialize)]
struct UploadCompleteRequest {
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct DownloadRequest {
    file_path: Option<String>,
    artifact_id: Option<String>,
    project_id: Option<String>,
}

#[derive(Deserialize)]
struct CleanupSessionRequest {
    session_id: String,
}

/// Main entry point for file transfer operations
pub async fn handle_file_transfer(
    operation: &str,
    data: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("File transfer operation: {}", operation);
    
    match operation {
        "upload_start" => start_upload(data, app_state).await,
        "upload_chunk" => receive_chunk(data, app_state).await,
        "upload_complete" => complete_upload(data, app_state).await,
        "download_request" => start_download(data, app_state).await,
        "cleanup_session" => cleanup_session(data, app_state).await,
        _ => {
            error!("Unknown file operation: {}", operation);
            Err(ApiError::bad_request(format!("Unknown file operation: {operation}")))
        }
    }
}

/// Initialize a new upload session
async fn start_upload(data: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: UploadStartRequest = serde_json::from_value(data)
        .map_err(|e| ApiError::bad_request(format!("Invalid upload start request: {e}")))?;
    
    info!("Starting upload for file: {} ({}) - project: {:?}", 
        request.filename, request.total_size, request.project_id);
    
    // Validate file size (500MB limit)
    const MAX_SIZE: usize = 500 * 1024 * 1024;
    if request.total_size > MAX_SIZE {
        return Err(ApiError::bad_request("File too large. Maximum size is 500MB"));
    }
    
    // Create session
    let session_id = Uuid::new_v4().to_string();
    let session = UploadSession {
        id: session_id.clone(),
        filename: request.filename.clone(),
        content_type: request.content_type,
        chunks: Vec::new(),
        total_size: request.total_size,
        received_size: 0,
    };
    
    app_state.upload_sessions.write().await.insert(session_id.clone(), session);
    
    info!("Upload session created: {}", session_id);
    
    Ok(WsServerMessage::Data {
        data: serde_json::json!({
            "type": "upload_started",
            "session_id": session_id,
            "filename": request.filename,
            "project_id": request.project_id,
        }),
        request_id: None,
    })
}

/// Receive a chunk of the file
async fn receive_chunk(data: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: UploadChunkRequest = serde_json::from_value(data)
        .map_err(|e| ApiError::bad_request(format!("Invalid chunk request: {e}")))?;
    
    debug!("Receiving chunk {} (final: {}) for session {}", 
        request.chunk_index, request.is_final, request.session_id);
    
    // Decode base64 chunk
    let chunk_data = BASE64.decode(&request.chunk)
        .map_err(|e| ApiError::bad_request(format!("Invalid base64 chunk: {e}")))?;
    
    // Update session
    let mut sessions = app_state.upload_sessions.write().await;
    let session = sessions.get_mut(&request.session_id)
        .ok_or_else(|| ApiError::not_found("Upload session not found"))?;
    
    // Store chunk
    while session.chunks.len() <= request.chunk_index {
        session.chunks.push(Vec::new());
    }
    session.chunks[request.chunk_index] = chunk_data.clone();
    session.received_size += chunk_data.len();
    
    let progress = (session.received_size as f64 / session.total_size as f64 * 100.0) as u8;
    
    debug!("Upload progress for {}: {}%", session.filename, progress);
    
    // If this is the final chunk, validate we have everything
    if request.is_final {
        let missing_chunks: Vec<usize> = (0..session.chunks.len())
            .filter(|i| session.chunks[*i].is_empty())
            .collect();
        
        if !missing_chunks.is_empty() {
            return Err(ApiError::bad_request(format!(
                "Final chunk received but missing chunks: {:?}",
                missing_chunks
            )));
        }
        
        debug!("Final chunk received and all chunks validated for {}", session.filename);
    }
    
    Ok(WsServerMessage::Data {
        data: serde_json::json!({
            "type": "chunk_received",
            "session_id": request.session_id,
            "chunk_index": request.chunk_index,
            "progress": progress,
            "is_final": request.is_final,
        }),
        request_id: None,
    })
}

/// Complete the upload and save the file
async fn complete_upload(data: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: UploadCompleteRequest = serde_json::from_value(data)
        .map_err(|e| ApiError::bad_request(format!("Invalid complete request: {e}")))?;
    
    info!("Completing upload for session {}", request.session_id);
    
    // Get and remove session
    let session = {
        let mut sessions = app_state.upload_sessions.write().await;
        sessions.remove(&request.session_id)
            .ok_or_else(|| ApiError::not_found("Upload session not found"))?
    };
    
    // Combine chunks
    let mut file_content = Vec::new();
    for chunk in session.chunks {
        file_content.extend_from_slice(&chunk);
    }
    
    // Verify size
    if file_content.len() != session.total_size {
        return Err(ApiError::bad_request(format!(
            "Size mismatch. Expected {} bytes, got {}",
            session.total_size,
            file_content.len()
        )));
    }
    
    // Save to ./uploads
    let upload_dir = std::path::Path::new("./uploads");
    std::fs::create_dir_all(upload_dir)
        .map_err(|e| ApiError::internal(format!("Failed to create upload directory: {e}")))?;
    
    let file_path = upload_dir.join(&session.filename);
    std::fs::write(&file_path, &file_content)
        .map_err(|e| ApiError::internal(format!("Failed to save file: {e}")))?;
    
    info!("File saved successfully: {}", file_path.display());
    
    Ok(WsServerMessage::Data {
        data: serde_json::json!({
            "type": "upload_complete",
            "filename": session.filename,
            "size": session.total_size,
            "path": file_path.to_string_lossy(),
        }),
        request_id: None,
    })
}

/// Start a file download
async fn start_download(data: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: DownloadRequest = serde_json::from_value(data)
        .map_err(|e| ApiError::bad_request(format!("Invalid download request: {e}")))?;
    
    debug!("Starting download: {:?}", request);
    
    // Determine what to download
    let file_content = if let Some(file_path) = request.file_path {
        // If project_id provided, scope to project upload directory
        let full_path = if let Some(project_id) = request.project_id {
            std::path::Path::new("./uploads")
                .join(&project_id)
                .join(&file_path)
        } else {
            std::path::Path::new(&file_path).to_path_buf()
        };
        
        // Download from file system
        std::fs::read(&full_path)
            .map_err(|e| ApiError::not_found(format!("File not found: {e}")))?
    } else if let Some(artifact_id) = request.artifact_id {
        // Download from artifact store
        let artifact = app_state.project_store
            .get_artifact(&artifact_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to get artifact: {e}")))?
            .ok_or_else(|| ApiError::not_found("Artifact not found"))?;
        
        artifact.content
            .ok_or_else(|| ApiError::not_found("Artifact has no content"))?
            .into_bytes()
    } else {
        return Err(ApiError::bad_request("Must specify either file_path or artifact_id"));
    };
    
    // Encode to base64 for transfer
    let encoded = BASE64.encode(&file_content);
    
    Ok(WsServerMessage::Data {
        data: serde_json::json!({
            "type": "download_ready",
            "content": encoded,
            "size": file_content.len(),
        }),
        request_id: None,
    })
}

/// Clean up an abandoned upload session
async fn cleanup_session(data: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: CleanupSessionRequest = serde_json::from_value(data)
        .map_err(|e| ApiError::bad_request(format!("Invalid cleanup request: {e}")))?;
    
    let mut sessions = app_state.upload_sessions.write().await;
    if sessions.remove(&request.session_id).is_some() {
        info!("Cleaned up upload session: {}", request.session_id);
        Ok(WsServerMessage::Status {
            message: format!("Session {} cleaned up", request.session_id),
            detail: None,
        })
    } else {
        Ok(WsServerMessage::Status {
            message: format!("Session {} not found", request.session_id),
            detail: None,
        })
    }
}
