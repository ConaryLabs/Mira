use axum::{
    extract::{State, Path, Json},
    response::IntoResponse,
    http::StatusCode,
};
use std::sync::Arc;
use crate::handlers::AppState;
use crate::git::GitRepoAttachment;
use serde::{Deserialize, Serialize};
use std::path::Path as StdPath;

#[derive(Deserialize)]
pub struct AttachRepoPayload {
    pub repo_url: String,
}

#[derive(Serialize)]
pub struct AttachRepoResponse {
    pub status: String,
    pub attachment: Option<GitRepoAttachment>,
    pub error: Option<String>,
}

pub async fn attach_repo_handler(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
    Json(payload): Json<AttachRepoPayload>,
) -> impl IntoResponse {
    let client = &state.git_client;
    match client.attach_repo(&project_id, &payload.repo_url).await {
        Ok(attachment) => {
            let client_clone = client.clone();
            let attachment_clone = attachment.clone();
            tokio::spawn(async move {
                let _ = client_clone.clone_repo(&attachment_clone).await;
                let _ = client_clone.import_codebase(&attachment_clone).await;
            });
            Json(AttachRepoResponse {
                status: "attached".to_string(),
                attachment: Some(attachment),
                error: None,
            })
        }
        Err(e) => Json(AttachRepoResponse {
            status: "error".to_string(),
            attachment: None,
            error: Some(e.to_string()),
        }),
    }
}

#[derive(Serialize)]
pub struct RepoListResponse {
    pub repos: Vec<GitRepoAttachment>,
}

pub async fn list_attached_repos_handler(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let store = &state.git_store;
    match store.get_attachments_for_project(&project_id).await {
        Ok(repos) => Json(RepoListResponse { repos }),
        Err(_) => Json(RepoListResponse { repos: Vec::new() }),
    }
}

#[derive(Deserialize)]
pub struct SyncRepoPayload {
    pub commit_message: String,
}

#[derive(Serialize)]
pub struct SyncRepoResponse {
    pub status: String,
    pub error: Option<String>,
}

// Full implementation without debug handler
pub async fn sync_repo_handler(
    State(app_state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Json(payload): Json<SyncRepoPayload>,
) -> Json<SyncRepoResponse> {
    let client = &app_state.git_client;
    let store = &app_state.git_store;
    
    // Get the attachment
    let attachment_result = store.get_attachment_by_id(&attachment_id).await;
    
    match attachment_result {
        Ok(Some(attachment)) => {
            // Check if attachment belongs to the project
            if attachment.project_id != project_id {
                return Json(SyncRepoResponse {
                    status: "not_found".to_string(),
                    error: Some("Attachment not found".to_string()),
                });
            }
            
            // Try to sync changes
            match client.sync_changes(&attachment, &payload.commit_message).await {
                Ok(_) => Json(SyncRepoResponse {
                    status: "synced".to_string(),
                    error: None,
                }),
                Err(e) => Json(SyncRepoResponse {
                    status: "error".to_string(),
                    error: Some(e.to_string()),
                }),
            }
        }
        Ok(None) => Json(SyncRepoResponse {
            status: "not_found".to_string(),
            error: Some("Attachment not found".to_string()),
        }),
        Err(e) => Json(SyncRepoResponse {
            status: "error".to_string(),
            error: Some(e.to_string()),
        }),
    }
}

// File tree and content handlers

#[derive(Serialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileNode>>,
}

#[derive(Serialize)]
pub struct FileTreeResponse {
    pub files: Vec<FileNode>,
}

#[derive(Serialize)]
pub struct FileContent {
    pub path: String,
    pub content: String,
    pub language: Option<String>,
    pub encoding: Option<String>,
}

pub async fn get_file_tree_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let store = &state.git_store;
    
    // Get the attachment to find the local path
    match store.get_attachment_by_id(&attachment_id).await {
        Ok(Some(attachment)) => {
            if attachment.project_id != project_id {
                return Json(FileTreeResponse { files: vec![] });
            }
            
            // Build file tree from the local repository
            let repo_path = StdPath::new(&attachment.local_path);
            let mut root_nodes = Vec::new();
            
            // Read directory structure
            if repo_path.exists() {
                if let Ok(entries) = std::fs::read_dir(repo_path) {
                    for entry in entries.flatten() {
                        if let Some(node) = build_file_node(&entry.path(), repo_path) {
                            root_nodes.push(node);
                        }
                    }
                }
            }
            
            Json(FileTreeResponse { files: root_nodes })
        }
        _ => Json(FileTreeResponse { files: vec![] }),
    }
}

fn build_file_node(path: &StdPath, base_path: &StdPath) -> Option<FileNode> {
    let name = path.file_name()?.to_str()?.to_string();
    
    // Skip hidden files and git directory
    if name.starts_with('.') {
        return None;
    }
    
    let relative_path = path.strip_prefix(base_path).ok()?;
    let path_str = relative_path.to_str()?.to_string();
    
    if path.is_dir() {
        let mut children = Vec::new();
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                if let Some(child) = build_file_node(&entry.path(), base_path) {
                    children.push(child);
                }
            }
        }
        Some(FileNode {
            name,
            path: path_str,
            node_type: "directory".to_string(),
            children: Some(children),
        })
    } else {
        Some(FileNode {
            name,
            path: path_str,
            node_type: "file".to_string(),
            children: None,
        })
    }
}

pub async fn get_file_content_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, file_path)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let store = &state.git_store;
    
    match store.get_attachment_by_id(&attachment_id).await {
        Ok(Some(attachment)) => {
            if attachment.project_id != project_id {
                return (StatusCode::NOT_FOUND, "File not found").into_response();
            }
            
            let full_path = StdPath::new(&attachment.local_path).join(&file_path);
            
            match std::fs::read_to_string(&full_path) {
                Ok(content) => {
                    let response = FileContent {
                        path: file_path.clone(),
                        content,
                        language: detect_language(&file_path),
                        encoding: Some("utf-8".to_string()),
                    };
                    Json(response).into_response()
                }
                Err(_) => (StatusCode::NOT_FOUND, "File not found").into_response(),
            }
        }
        _ => (StatusCode::NOT_FOUND, "Repository not found").into_response(),
    }
}

pub async fn update_file_content_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, file_path)): Path<(String, String, String)>,
    Json(payload): Json<UpdateFilePayload>,
) -> impl IntoResponse {
    let store = &state.git_store;
    
    match store.get_attachment_by_id(&attachment_id).await {
        Ok(Some(attachment)) => {
            if attachment.project_id != project_id {
                return (StatusCode::NOT_FOUND, "File not found").into_response();
            }
            
            let full_path = StdPath::new(&attachment.local_path).join(&file_path);
            
            // Write the updated content
            match std::fs::write(&full_path, &payload.content) {
                Ok(_) => {
                    let response = FileContent {
                        path: file_path.clone(),
                        content: payload.content,
                        language: detect_language(&file_path),
                        encoding: Some("utf-8".to_string()),
                    };
                    Json(response).into_response()
                }
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write file: {}", e)).into_response()
                }
            }
        }
        _ => (StatusCode::NOT_FOUND, "Repository not found").into_response(),
    }
}

#[derive(Deserialize)]
pub struct UpdateFilePayload {
    pub content: String,
}

fn detect_language(path: &str) -> Option<String> {
    let ext = StdPath::new(path).extension()?.to_str()?;
    let language = match ext {
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "cpp" | "cc" | "cxx" => "cpp",
        "c" | "h" => "c",
        "md" => "markdown",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "html" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        _ => return None,
    };
    Some(language.to_string())
}
