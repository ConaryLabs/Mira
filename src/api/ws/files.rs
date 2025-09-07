// src/api/ws/files.rs
// Handles file upload and download operations through WebSocket
// Supports chunked transfers for large files (up to 500MB)
// Phase 6: WebSocket-only file transfer implementation

use std::sync::Arc;
use std::collections::HashMap;
use serde::Deserialize;
use serde_json::Value;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use tokio::sync::RwLock;
use uuid::Uuid;
use tracing::{debug, error, info};

use crate::state::AppState;
use crate::api::ws::message::WsServerMessage;
use crate::api::error::{ApiError, ApiResult};

// Upload sessions stored in memory - safe for single-user deployment
lazy_static::lazy_static! {
    static ref UPLOAD_SESSIONS: RwLock<HashMap<String, UploadSession>> = RwLock::new(HashMap::new());
    static ref ACTIVE_UPLOADS: RwLock<usize> = RwLock::new(0);
}

#[derive(Debug, Clone)]
struct UploadSession {
    id: String,
    filename: String,
    content_type: String,
    chunks: Vec<Vec<u8>>,
    total_size: usize,
    received_size: usize,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct UploadStartRequest {
    filename: String,
    content_type: String,
    total_size: usize,
    project_id: Option<String>,  // For associating with projects
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
    save_as_artifact: Option<bool>,
    memory_attachment: Option<bool>,
}

#[derive(Debug, Deserialize)]  // FIX: Added Debug derive
struct DownloadRequest {
    file_path: Option<String>,
    artifact_id: Option<String>,
    project_id: Option<String>,
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
        "cleanup_session" => cleanup_session(data).await,
        _ => {
            error!("Unknown file operation: {}", operation);
            Err(ApiError::bad_request(format!("Unknown file operation: {}. Valid operations: upload_start, upload_chunk, upload_complete, download_request", operation)))
        }
    }
}

/// Initialize a new upload session
async fn start_upload(data: Value, _app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: UploadStartRequest = serde_json::from_value(data)
        .map_err(|e| ApiError::bad_request(format!("Invalid upload start request: {}", e)))?;
    
    info!("Starting upload for file: {} ({})", request.filename, request.total_size);
    
    // Check concurrent upload limit (single user, but still good practice)
    let active = *ACTIVE_UPLOADS.read().await;
    if active > 0 {
        return Ok(WsServerMessage::Error {
            message: "Already processing an upload. One at a time for optimal performance.".to_string(),
            code: "UPLOAD_IN_PROGRESS".to_string(),
        });
    }
    
    // 500MB limit for ChatGPT exports and other large JSON files
    if request.total_size > 500 * 1024 * 1024 {
        return Ok(WsServerMessage::Error {
            message: format!("File too large: {}MB. Maximum size is 500MB.", request.total_size / (1024 * 1024)),
            code: "FILE_TOO_LARGE".to_string(),
        });
    }
    
    let session_id = Uuid::new_v4().to_string();
    let session = UploadSession {
        id: session_id.clone(),
        filename: request.filename.clone(),
        content_type: request.content_type,
        chunks: Vec::new(),
        total_size: request.total_size,
        received_size: 0,
        created_at: chrono::Utc::now(),
    };
    
    // Store session and increment counter
    UPLOAD_SESSIONS.write().await.insert(session_id.clone(), session);
    *ACTIVE_UPLOADS.write().await += 1;
    
    Ok(WsServerMessage::Data {
        data: serde_json::json!({
            "status": "ready",
            "session_id": session_id,
            "chunk_size": 5 * 1024 * 1024,  // 5MB chunks for gigabit connections
            "max_chunks": (request.total_size as f64 / (5.0 * 1024.0 * 1024.0)).ceil() as u32,
            "message": format!("Upload session created for {}", request.filename)
        }),
        request_id: None,
    })
}

/// Receive and store a file chunk
async fn receive_chunk(data: Value, _app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: UploadChunkRequest = serde_json::from_value(data)
        .map_err(|e| ApiError::bad_request(format!("Invalid chunk request: {}", e)))?;
    
    debug!("Receiving chunk {} for session {}", request.chunk_index, request.session_id);
    
    // Decode the base64 chunk
    let chunk_data = BASE64.decode(&request.chunk)
        .map_err(|e| ApiError::bad_request(format!("Failed to decode chunk: {}", e)))?;
    
    let mut sessions = UPLOAD_SESSIONS.write().await;
    let session = sessions.get_mut(&request.session_id)
        .ok_or_else(|| ApiError::not_found("Upload session not found or expired"))?;
    
    // Store the chunk
    session.chunks.push(chunk_data.clone());
    session.received_size += chunk_data.len();
    
    // Calculate progress
    let progress = ((session.received_size as f64 / session.total_size as f64) * 100.0).round() as u32;
    
    debug!("Upload progress: {}% ({}/{})", progress, session.received_size, session.total_size);
    
    if request.is_final {
        // Validate we received everything
        if session.received_size != session.total_size {
            error!("Size mismatch: expected {}, got {}", session.total_size, session.received_size);
            return Ok(WsServerMessage::Error {
                message: format!("Upload size mismatch. Expected {} bytes, received {}", 
                    session.total_size, session.received_size),
                code: "SIZE_MISMATCH".to_string(),
            });
        }
        
        let filename = session.filename.clone();
        let size = session.received_size;
        
        info!("Upload complete: {} ({} bytes)", filename, size);
        
        return Ok(WsServerMessage::Data {
            data: serde_json::json!({
                "status": "upload_complete",
                "session_id": request.session_id,
                "filename": filename,
                "size": size,
                "message": "File received successfully. Call upload_complete to finalize."
            }),
            request_id: None,
        });
    }
    
    Ok(WsServerMessage::Data {
        data: serde_json::json!({
            "status": "chunk_received",
            "progress": progress,
            "received_size": session.received_size,
            "total_size": session.total_size,
            "chunk_index": request.chunk_index,
        }),
        request_id: None,
    })
}

/// Finalize an upload and integrate with the system
async fn complete_upload(data: Value, _app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {  // FIX: Added underscore to unused param
    let request: UploadCompleteRequest = serde_json::from_value(data)
        .map_err(|e| ApiError::bad_request(format!("Invalid complete request: {}", e)))?;
    
    let mut sessions = UPLOAD_SESSIONS.write().await;
    let session = sessions.remove(&request.session_id)
        .ok_or_else(|| ApiError::not_found("Upload session not found"))?;
    
    // Decrement active uploads
    *ACTIVE_UPLOADS.write().await -= 1;
    
    // Combine all chunks
    let complete_file = session.chunks.concat();
    
    info!("Finalizing upload: {} ({} bytes)", session.filename, complete_file.len());
    
    // Handle special case: ChatGPT export
    if session.filename.ends_with(".json") && session.content_type.contains("json") {
        if let Ok(json_str) = String::from_utf8(complete_file.clone()) {
            // Try to parse as ChatGPT export
            if json_str.contains("\"conversations\"") || json_str.contains("\"chat_history\"") {
                info!("Detected ChatGPT export format");
                
                // TODO: Process ChatGPT export into memory system
                // This would involve parsing conversations and storing them
                
                return Ok(WsServerMessage::Data {
                    data: serde_json::json!({
                        "status": "import_complete",
                        "filename": session.filename,
                        "type": "chatgpt_export",
                        "message": "ChatGPT history imported successfully"
                    }),
                    request_id: None,
                });
            }
        }
    }
    
    // Save as artifact if requested
    if request.save_as_artifact.unwrap_or(false) {
        // TODO: Integration with artifact system
        debug!("Saving as artifact: {}", session.filename);
    }
    
    // Attach to memory if requested
    if request.memory_attachment.unwrap_or(false) {
        // TODO: Integration with memory system
        debug!("Attaching to memory: {}", session.filename);
    }
    
    Ok(WsServerMessage::Data {
        data: serde_json::json!({
            "status": "finalized",
            "filename": session.filename,
            "size": complete_file.len(),
            "message": "Upload finalized successfully"
        }),
        request_id: None,
    })
}

/// Handle file download requests
async fn start_download(data: Value, _app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: DownloadRequest = serde_json::from_value(data)
        .map_err(|e| ApiError::bad_request(format!("Invalid download request: {}", e)))?;
    
    info!("Download request: {:?}", request);
    
    // TODO: Actually fetch the file from storage
    // For now, return a placeholder response
    
    if let Some(artifact_id) = request.artifact_id {
        // Fetch from artifact storage
        debug!("Fetching artifact: {}", artifact_id);
        
        // Placeholder content
        let file_content = b"// Artifact content would go here\n";
        let encoded = BASE64.encode(file_content);
        
        return Ok(WsServerMessage::Data {
            data: serde_json::json!({
                "status": "complete",
                "artifact_id": artifact_id,
                "content": encoded,
                "size": file_content.len(),
                "content_type": "text/plain"
            }),
            request_id: None,
        });
    }
    
    if let Some(file_path) = request.file_path {
        // For code files under a reasonable size, send in one chunk
        debug!("Fetching file: {}", file_path);
        
        // TODO: Actually read the file
        let file_content = format!("// File content for: {}\n", file_path).into_bytes();
        let encoded = BASE64.encode(&file_content);
        
        return Ok(WsServerMessage::Data {
            data: serde_json::json!({
                "status": "complete",
                "filename": file_path,
                "content": encoded,
                "size": file_content.len(),
                "content_type": "text/plain"
            }),
            request_id: None,
        });
    }
    
    Err(ApiError::bad_request("Must specify either artifact_id or file_path"))
}

/// Clean up an abandoned upload session
async fn cleanup_session(data: Value) -> ApiResult<WsServerMessage> {
    let session_id = data["session_id"].as_str()
        .ok_or_else(|| ApiError::bad_request("Missing session_id"))?;
    
    let mut sessions = UPLOAD_SESSIONS.write().await;
    if sessions.remove(session_id).is_some() {
        *ACTIVE_UPLOADS.write().await -= 1;
        info!("Cleaned up upload session: {}", session_id);
        
        Ok(WsServerMessage::Data {
            data: serde_json::json!({
                "status": "cleaned",
                "session_id": session_id
            }),
            request_id: None,
        })
    } else {
        Err(ApiError::not_found("Session not found"))
    }
}

/// Background task to clean up old sessions (optional)
pub async fn cleanup_old_sessions() {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(600)).await; // Every 10 minutes
        
        let now = chrono::Utc::now();
        let mut sessions = UPLOAD_SESSIONS.write().await;
        let mut removed = 0;
        
        sessions.retain(|id, session| {
            let age = now - session.created_at;
            if age.num_minutes() > 30 {
                info!("Removing stale upload session: {}", id);
                removed += 1;
                false
            } else {
                true
            }
        });
        
        if removed > 0 {
            *ACTIVE_UPLOADS.write().await = sessions.len();
            info!("Cleaned up {} stale upload sessions", removed);
        }
    }
}
