// src/api/ws/filesystem.rs
// WebSocket handler for file system operations

use std::sync::Arc;
use std::fs;
use std::path::Path;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{info, warn, error, debug};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    state::AppState,
    utils::is_path_allowed,  // Now using the utility function
};

#[derive(Debug, Deserialize)]
struct SaveFileRequest {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ReadFileRequest {
    path: String,
}

#[derive(Debug, Deserialize)]
struct ListFilesRequest {
    path: String,
    recursive: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct DeleteFileRequest {
    path: String,
}

/// Main entry point for all filesystem-related WebSocket commands
pub async fn handle_filesystem_command(
    method: &str,
    params: Value,
    _app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("Processing filesystem command: {} with params: {:?}", method, params);
    
    let result = match method {
        "file.save" => save_file(params).await,
        "files.write" => save_file(params).await,
        "file.read" => read_file(params).await,
        "file.list" => list_files(params).await,
        "file.delete" => delete_file(params).await,
        "file.exists" => check_file_exists(params).await,
        _ => {
            error!("Unknown filesystem method: {}", method);
            return Err(ApiError::bad_request(format!("Unknown filesystem method: {method}")));
        }
    };
    
    match &result {
        Ok(_) => info!("Successfully processed filesystem command: {}", method),
        Err(e) => error!("Failed to process filesystem command {}: {:?}", method, e),
    }
    
    result
}

/// Save content to a file
async fn save_file(params: Value) -> ApiResult<WsServerMessage> {
    let req: SaveFileRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid save file request: {e}")))?;
    
    info!("Saving file to: {}", req.path);
    
    // Validate path using utility function
    let path = Path::new(&req.path);
    if !is_path_allowed(path) {
        return Err(ApiError::forbidden("Path outside allowed directories"));
    }
    
    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| ApiError::internal(format!("Failed to create directories: {e}")))?;
    }
    
    // Write file
    fs::write(path, &req.content)
        .map_err(|e| ApiError::internal(format!("Failed to write file: {e}")))?;
    
    info!("Successfully saved file to {}", req.path);
    
    Ok(WsServerMessage::Status {
        message: format!("Saved to {}", req.path),
        detail: None,
    })
}

/// Read content from a file
async fn read_file(params: Value) -> ApiResult<WsServerMessage> {
    let req: ReadFileRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid read file request: {e}")))?;
    
    debug!("Reading file from: {}", req.path);
    
    // Validate path using utility function
    let path = Path::new(&req.path);
    if !is_path_allowed(path) {
        return Err(ApiError::forbidden("Path outside allowed directories"));
    }
    
    if !path.exists() {
        return Err(ApiError::not_found(format!("File not found: {}", req.path)));
    }
    
    // Read file content
    let content = fs::read_to_string(path)
        .map_err(|e| ApiError::internal(format!("Failed to read file: {e}")))?;
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "file_content",
            "path": req.path,
            "content": content,
        }),
        request_id: None,
    })
}

/// List files in a directory
async fn list_files(params: Value) -> ApiResult<WsServerMessage> {
    let req: ListFilesRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid list files request: {e}")))?;
    
    debug!("Listing files in: {}", req.path);
    
    let path = Path::new(&req.path);
    if !is_path_allowed(path) {
        return Err(ApiError::forbidden("Path outside allowed directories"));
    }
    
    if !path.exists() {
        return Err(ApiError::not_found(format!("Directory not found: {}", req.path)));
    }
    
    if !path.is_dir() {
        return Err(ApiError::bad_request(format!("{} is not a directory", req.path)));
    }
    
    let mut files = Vec::new();
    let recursive = req.recursive.unwrap_or(false);
    
    if recursive {
        collect_files_recursive(path, &mut files)?;
    } else {
        for entry in fs::read_dir(path)
            .map_err(|e| ApiError::internal(format!("Failed to read directory: {e}")))? 
        {
            let entry = entry.map_err(|e| ApiError::internal(format!("Failed to read entry: {e}")))?;
            let metadata = entry.metadata()
                .map_err(|e| ApiError::internal(format!("Failed to read metadata: {e}")))?;
            
            files.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "path": entry.path().to_string_lossy(),
                "is_file": metadata.is_file(),
                "is_dir": metadata.is_dir(),
                "size": metadata.len(),
            }));
        }
    }
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "file_list",
            "path": req.path,
            "files": files,
            "count": files.len(),
        }),
        request_id: None,
    })
}

/// Delete a file
async fn delete_file(params: Value) -> ApiResult<WsServerMessage> {
    let req: DeleteFileRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid delete file request: {e}")))?;
    
    warn!("Deleting file: {}", req.path);
    
    let path = Path::new(&req.path);
    if !is_path_allowed(path) {
        return Err(ApiError::forbidden("Path outside allowed directories"));
    }
    
    if !path.exists() {
        return Err(ApiError::not_found(format!("File not found: {}", req.path)));
    }
    
    // Delete file or directory
    if path.is_file() {
        fs::remove_file(path)
            .map_err(|e| ApiError::internal(format!("Failed to delete file: {e}")))?;
    } else {
        fs::remove_dir_all(path)
            .map_err(|e| ApiError::internal(format!("Failed to delete directory: {e}")))?;
    }
    
    warn!("Successfully deleted: {}", req.path);
    
    Ok(WsServerMessage::Status {
        message: format!("Deleted {}", req.path),
        detail: None,
    })
}

/// Check if a file exists
async fn check_file_exists(params: Value) -> ApiResult<WsServerMessage> {
    let path = params["path"].as_str()
        .ok_or_else(|| ApiError::bad_request("Missing path"))?;
    
    let file_path = Path::new(path);
    if !is_path_allowed(file_path) {
        return Err(ApiError::forbidden("Path outside allowed directories"));
    }
    
    let exists = file_path.exists();
    let is_file = if exists { file_path.is_file() } else { false };
    let is_dir = if exists { file_path.is_dir() } else { false };
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "file_exists",
            "path": path,
            "exists": exists,
            "is_file": is_file,
            "is_dir": is_dir,
        }),
        request_id: None,
    })
}

/// Helper: Recursively collect files
fn collect_files_recursive(dir: &Path, files: &mut Vec<Value>) -> ApiResult<()> {
    for entry in fs::read_dir(dir)
        .map_err(|e| ApiError::internal(format!("Failed to read directory: {e}")))? 
    {
        let entry = entry.map_err(|e| ApiError::internal(format!("Failed to read entry: {e}")))?;
        let metadata = entry.metadata()
            .map_err(|e| ApiError::internal(format!("Failed to read metadata: {e}")))?;
        
        files.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry.path().to_string_lossy(),
            "is_file": metadata.is_file(),
            "is_dir": metadata.is_dir(),
            "size": metadata.len(),
        }));
        
        // Recurse into subdirectories
        if metadata.is_dir() {
            collect_files_recursive(&entry.path(), files)?;
        }
    }
    
    Ok(())
}
