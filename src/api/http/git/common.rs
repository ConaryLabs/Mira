// src/api/http/git/common.rs
// MIGRATED: Updated to use unified ApiError and IntoApiError pattern
// CRITICAL: Common utilities for Git HTTP handlers with consistent error handling

use std::path::Path;

use crate::git::types::GitRepoAttachment;
use crate::git::store::GitStore;
use crate::api::error::{ApiError, ApiResult, IntoApiError, IntoApiErrorOption};

/// Validate and retrieve an attachment, ensuring it belongs to the specified project
/// MIGRATED: Now uses unified error handling instead of manual StatusCode responses
pub async fn get_validated_attachment(
    store: &GitStore,
    project_id: &str,
    attachment_id: &str,
) -> ApiResult<GitRepoAttachment> {
    let attachment = store
        .get_attachment_by_id(attachment_id)
        .await
        .into_api_error("Failed to retrieve attachment")?
        .ok_or_not_found("Attachment not found")?;
    
    // Validate that attachment belongs to the specified project
    if attachment.project_id != project_id {
        return Err(ApiError::forbidden("Attachment belongs to different project"));
    }
    
    Ok(attachment)
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
