// src/api/http/git.rs

use axum::{
    extract::{State, Path, Json, Query},
    response::IntoResponse,
    http::StatusCode,
};
use std::sync::Arc;
use crate::handlers::AppState;
use crate::git::{GitRepoAttachment, BranchInfo, CommitInfo, DiffInfo};
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

#[derive(Serialize, Debug, Clone)]
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

// UPDATED: Now with debug logging and fixed tree building
pub async fn get_file_tree_handler(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    eprintln!("🌳 Getting file tree for project {} attachment {}", project_id, attachment_id);
    
    let store = &state.git_store;
    
    // Get the attachment to verify it belongs to the project
    match store.get_attachment_by_id(&attachment_id).await {
        Ok(Some(attachment)) => {
            if attachment.project_id != project_id {
                eprintln!("❌ Attachment doesn't belong to project");
                return Json(FileTreeResponse { files: vec![] });
            }
            
            // Use the new git client method to get the file tree
            match state.git_client.get_file_tree(&attachment) {
                Ok(git_nodes) => {
                    eprintln!("✅ Got {} nodes from git", git_nodes.len());
                    
                    // Convert from git FileNode to our API FileNode format
                    let files = convert_git_nodes_to_api_nodes(git_nodes);
                    
                    eprintln!("📁 Returning {} root nodes", files.len());
                    Json(FileTreeResponse { files })
                }
                Err(e) => {
                    eprintln!("❌ Failed to get file tree: {}", e);
                    Json(FileTreeResponse { files: vec![] })
                }
            }
        }
        _ => {
            eprintln!("❌ Attachment not found");
            Json(FileTreeResponse { files: vec![] })
        }
    }
}

// Helper function to convert flat git nodes to nested tree structure
fn convert_git_nodes_to_api_nodes(git_nodes: Vec<crate::git::FileNode>) -> Vec<FileNode> {
    use std::collections::HashMap;
    
    #[derive(Debug)]
    struct TreeBuilder {
        nodes: HashMap<String, FileNode>,
        children: HashMap<String, Vec<String>>,
    }
    
    impl TreeBuilder {
        fn new() -> Self {
            Self {
                nodes: HashMap::new(),
                children: HashMap::new(),
            }
        }
        
        fn add_node(&mut self, git_node: crate::git::FileNode) {
            // Skip .git directory
            if git_node.path.starts_with(".git") {
                return;
            }
            
            // The git client returns FileNode with node_type as FileNodeType enum
            // We need to convert it to string for the API
            let node_type_str = match git_node.node_type {
                crate::git::FileNodeType::File => "file".to_string(),
                crate::git::FileNodeType::Directory => "directory".to_string(),
            };
            
            let api_node = FileNode {
                name: git_node.name.clone(),
                path: git_node.path.clone(),
                node_type: node_type_str.clone(),
                children: None,
            };
            
            // Store the node
            self.nodes.insert(git_node.path.clone(), api_node);
            
            // Register with parent
            if let Some(parent_path) = self.get_parent_path(&git_node.path) {
                self.children.entry(parent_path)
                    .or_insert_with(Vec::new)
                    .push(git_node.path);
            }
        }
        
        fn get_parent_path(&self, path: &str) -> Option<String> {
            path.rfind('/').map(|pos| path[..pos].to_string())
        }
        
        fn build_tree(&mut self) -> Vec<FileNode> {
            let mut roots = Vec::new();
            
            // Find root nodes
            for path in self.nodes.keys() {
                if !path.contains('/') {
                    roots.push(path.clone());
                }
            }
            
            // Build tree for each root
            roots.into_iter()
                .filter_map(|path| self.build_node_tree(&path))
                .collect()
        }
        
        fn build_node_tree(&mut self, path: &str) -> Option<FileNode> {
            let mut node = self.nodes.get(path)?.clone();
            
            if node.node_type == "directory" {
                if let Some(child_paths) = self.children.get(path) {
                    // Clone the child paths to avoid borrow checker issues
                    let child_paths_vec: Vec<String> = child_paths.clone();
                    let mut children: Vec<FileNode> = child_paths_vec.iter()
                        .filter_map(|child_path| self.build_node_tree(child_path))
                        .collect();
                    
                    // Sort: directories first, then files, alphabetically
                    children.sort_by(|a, b| {
                        match (a.node_type.as_str(), b.node_type.as_str()) {
                            ("directory", "file") => std::cmp::Ordering::Less,
                            ("file", "directory") => std::cmp::Ordering::Greater,
                            _ => a.name.cmp(&b.name),
                        }
                    });
                    
                    node.children = Some(children);
                } else {
                    node.children = Some(Vec::new());
                }
            }
            
            Some(node)
        }
    }
    
    let mut builder = TreeBuilder::new();
    
    // Add all nodes to the builder
    for git_node in git_nodes {
        builder.add_node(git_node);
    }
    
    // Build and return the tree
    let mut tree = builder.build_tree();
    
    // Sort root level
    tree.sort_by(|a, b| {
        match (a.node_type.as_str(), b.node_type.as_str()) {
            ("directory", "file") => std::cmp::Ordering::Less,
            ("file", "directory") => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });
    
    tree
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

// ===== NEW PHASE 3 ENDPOINTS =====

#[derive(Serialize)]
pub struct BranchListResponse {
    pub branches: Vec<BranchInfo>,
}

#[derive(Deserialize)]
pub struct SwitchBranchRequest {
    pub branch_name: String,
}

#[derive(Deserialize)]
pub struct CommitHistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Serialize)]
pub struct CommitHistoryResponse {
    pub commits: Vec<CommitInfo>,
}

#[derive(Serialize)]
pub struct DiffResponse {
    pub diff: DiffInfo,
}

#[derive(Deserialize)]
pub struct FileContentQuery {
    pub commit_id: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct FileContentAtCommitResponse {
    pub content: String,
    pub path: String,
    pub commit_id: String,
}

/// GET /projects/:project_id/git/:attachment_id/branches
/// List all branches in the repository
pub async fn list_branches(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let attachment = match state.git_store.get_attachment_by_id(&attachment_id).await {
        Ok(Some(att)) if att.project_id == project_id => att,
        Ok(_) => return (StatusCode::NOT_FOUND, "Attachment not found").into_response(),
        Err(e) => {
            eprintln!("Failed to get attachment: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    match state.git_client.get_branches(&attachment) {
        Ok(branches) => {
            Json(BranchListResponse { branches }).into_response()
        }
        Err(e) => {
            eprintln!("Failed to list branches: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list branches").into_response()
        }
    }
}

/// POST /projects/:project_id/git/:attachment_id/branches/switch
/// Switch to a different branch
pub async fn switch_branch(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Json(req): Json<SwitchBranchRequest>,
) -> impl IntoResponse {
    let attachment = match state.git_store.get_attachment_by_id(&attachment_id).await {
        Ok(Some(att)) if att.project_id == project_id => att,
        Ok(_) => return (StatusCode::NOT_FOUND, "Attachment not found").into_response(),
        Err(e) => {
            eprintln!("Failed to get attachment: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    match state.git_client.switch_branch(&attachment, &req.branch_name) {
        Ok(()) => {
            (StatusCode::OK, Json(serde_json::json!({
                "success": true,
                "message": format!("Switched to branch '{}'", req.branch_name)
            }))).into_response()
        }
        Err(e) => {
            eprintln!("Failed to switch branch: {}", e);
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": format!("Failed to switch branch: {}", e)
            }))).into_response()
        }
    }
}

/// GET /projects/:project_id/git/:attachment_id/commits
/// Get commit history
pub async fn get_commit_history(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Query(params): Query<CommitHistoryQuery>,
) -> impl IntoResponse {
    let attachment = match state.git_store.get_attachment_by_id(&attachment_id).await {
        Ok(Some(att)) if att.project_id == project_id => att,
        Ok(_) => return (StatusCode::NOT_FOUND, "Attachment not found").into_response(),
        Err(e) => {
            eprintln!("Failed to get attachment: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    match state.git_client.get_commits(&attachment, params.limit) {
        Ok(commits) => {
            Json(CommitHistoryResponse { commits }).into_response()
        }
        Err(e) => {
            eprintln!("Failed to get commit history: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read commit history").into_response()
        }
    }
}

/// GET /projects/:project_id/git/:attachment_id/commits/:commit_id/diff
/// Get diff for a specific commit
pub async fn get_commit_diff(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id, commit_id)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let attachment = match state.git_store.get_attachment_by_id(&attachment_id).await {
        Ok(Some(att)) if att.project_id == project_id => att,
        Ok(_) => return (StatusCode::NOT_FOUND, "Attachment not found").into_response(),
        Err(e) => {
            eprintln!("Failed to get attachment: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    match state.git_client.get_diff(&attachment, &commit_id) {
        Ok(diff) => {
            Json(DiffResponse { diff }).into_response()
        }
        Err(e) => {
            eprintln!("Failed to get diff: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate diff").into_response()
        }
    }
}

/// GET /projects/:project_id/git/:attachment_id/file_at_commit
/// Get file content at a specific commit
pub async fn get_file_at_commit(
    State(state): State<Arc<AppState>>,
    Path((project_id, attachment_id)): Path<(String, String)>,
    Query(params): Query<FileContentQuery>,
) -> impl IntoResponse {
    let attachment = match state.git_store.get_attachment_by_id(&attachment_id).await {
        Ok(Some(att)) if att.project_id == project_id => att,
        Ok(_) => return (StatusCode::NOT_FOUND, "Attachment not found").into_response(),
        Err(e) => {
            eprintln!("Failed to get attachment: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    match state.git_client.get_file_at_commit(&attachment, &params.commit_id, &params.path) {
        Ok(content) => {
            Json(FileContentAtCommitResponse {
                content,
                path: params.path,
                commit_id: params.commit_id,
            }).into_response()
        }
        Err(e) => {
            eprintln!("Failed to get file content: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file").into_response()
        }
    }
}
