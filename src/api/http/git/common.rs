// src/api/http/git/common.rs
// Common utilities for Git HTTP handlers

use axum::{http::StatusCode, response::{IntoResponse, Response}};
use crate::git::types::GitRepoAttachment;
use crate::git::store::GitStore;
use std::path::Path;

/// Validate and retrieve an attachment, ensuring it belongs to the specified project
pub async fn get_validated_attachment(
    store: &GitStore,
    project_id: &str,
    attachment_id: &str,
) -> Result<GitRepoAttachment, Response> {
    match store.get_attachment_by_id(attachment_id).await {
        Ok(Some(att)) if att.project_id == project_id => Ok(att),
        Ok(Some(_)) => Err((StatusCode::FORBIDDEN, "Attachment belongs to different project").into_response()),
        Ok(None) => Err((StatusCode::NOT_FOUND, "Attachment not found").into_response()),
        Err(e) => {
            tracing::error!("Failed to get attachment: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response())
        }
    }
}

/// Detect programming language from file extension
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
        "vue" => "vue",
        "svelte" => "svelte",
        _ => return None,
    };
    Some(language.to_string())
}

/// Check if a file should be indexed based on path and size
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
       path_str.contains("/build/") ||
       path_str.contains("/vendor/") ||
       path_str.contains("/.env") {
        return false;
    }
    
    // Check file size (skip files larger than 500KB)
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.len() > 500_000 {
            return false;
        }
    }
    
    // Check extension - only index text-based files
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(
            ext.to_lowercase().as_str(),
            "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "cpp" | "c" | "h" |
            "hpp" | "cc" | "cxx" | "php" | "rb" | "swift" | "kt" | "scala" | "r" | "lua" |
            "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" |
            "json" | "yaml" | "yml" | "toml" | "xml" | "ini" | "cfg" | "conf" |
            "md" | "txt" | "rst" | "tex" | "html" | "css" | "scss" | "sass" | "less" |
            "sql" | "graphql" | "proto" | "vue" | "svelte" | "elm" | "clj" | "ex" | "exs"
        )
    } else {
        false
    }
}
