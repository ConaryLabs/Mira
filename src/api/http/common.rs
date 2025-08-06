// src/api/http/git/common.rs
use axum::{http::StatusCode, response::IntoResponse};
use crate::git::types::GitRepoAttachment;
use crate::git::store::GitStore;
use std::path::Path;

pub async fn get_validated_attachment(
    store: &GitStore,
    project_id: &str,
    attachment_id: &str,
) -> Result<GitRepoAttachment, impl IntoResponse> {
    match store.get_attachment_by_id(attachment_id).await {
        Ok(Some(att)) if att.project_id == project_id => Ok(att),
        Ok(_) => Err((StatusCode::NOT_FOUND, "Attachment not found").into_response()),
        Err(e) => {
            eprintln!("Failed to get attachment: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response())
        }
    }
}

pub fn detect_language(path: &str) -> Option<String> {
    let ext = Path::new(path).extension()?.to_str()?;
    
    let language = match ext.to_lowercase().as_str() {
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
        "sql" => "sql",
        "sh" | "bash" => "shell",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "r" => "r",
        "lua" => "lua",
        "xml" => "xml",
        _ => return None,
    };
    Some(language.to_string())
}

pub fn should_index_file(path: &Path) -> bool {
    // Skip hidden files and common ignore patterns
    let path_str = path.to_string_lossy();
    if path_str.contains("/.git/") || 
       path_str.contains("/node_modules/") ||
       path_str.contains("/target/") ||
       path_str.contains("/__pycache__/") ||
       path_str.contains("/.vscode/") ||
       path_str.contains("/.idea/") ||
       path_str.contains("/dist/") ||
       path_str.contains("/build/") {
        return false;
    }
    
    // Check file size (skip files larger than 500KB)
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.len() > 500_000 {
            return false;
        }
    }
    
    // Check extension
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(ext.to_lowercase().as_str(),
            "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "java" | 
            "cpp" | "c" | "h" | "hpp" | "go" | "rb" | "php" | "swift" | 
            "kt" | "kts" | "scala" | "r" | "lua" | "sql" | "sh" | "bash" |
            "md" | "txt" | "json" | "yaml" | "yml" | "toml" | "xml" | "html" |
            "css" | "scss" | "sass" | "less"
        )
    } else {
        // Check for common extensionless files
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            matches!(file_name, "Makefile" | "Dockerfile" | "README" | "LICENSE")
        } else {
            false
        }
    }
}
