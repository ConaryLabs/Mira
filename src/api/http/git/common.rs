// src/api/http/git/common.rs
// Complete migration to unified ApiError pattern

use std::path::Path;

use crate::git::types::GitRepoAttachment;
use crate::git::store::GitStore;
use crate::api::error::{ApiError, ApiResult, IntoApiError, IntoApiErrorOption};

/// Validate and retrieve an attachment, ensuring it belongs to the specified project
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
        _ => return None,
    };
    Some(language.to_string())
}

/// Check if a file should be indexed (avoid binary and ignored files)
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
    
    // Check extension - only index known text file types
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(
            ext.to_lowercase().as_str(),
            "rs" | "js" | "ts" | "jsx" | "tsx" | "py" | "go" | "java" | "cpp" | "cc" |
            "cxx" | "c" | "h" | "md" | "json" | "yaml" | "yml" | "toml" | "html" |
            "css" | "scss" | "sass" | "sql" | "sh" | "bash" | "rb" | "php" |
            "swift" | "kt" | "kts" | "scala" | "r" | "lua" | "xml" | "txt" |
            "cfg" | "ini" | "env" | "gitignore" | "dockerfile" | "makefile"
        )
    } else {
        // Files without extensions - check common names
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            matches!(
                name.to_lowercase().as_str(),
                "readme" | "license" | "changelog" | "dockerfile" | "makefile" |
                "cargo.toml" | "package.json" | "requirements.txt" | "go.mod" |
                "pom.xml" | "build.gradle" | ".gitignore" | ".env"
            )
        } else {
            false
        }
    }
}

/// Check if a file path contains sensitive information that should be excluded
pub fn is_sensitive_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    
    // Check for sensitive file patterns
    path_str.contains("secret") ||
    path_str.contains("password") ||
    path_str.contains("private") ||
    path_str.contains("credential") ||
    path_str.contains("token") ||
    path_str.contains("key") ||
    path_str.contains(".pem") ||
    path_str.contains(".key") ||
    path_str.contains(".p12") ||
    path_str.contains(".pfx") ||
    path_str.ends_with(".env") ||
    path_str.ends_with(".env.local") ||
    path_str.ends_with(".env.production") ||
    path_str.ends_with(".env.development")
}

/// Get file size in human-readable format
pub fn format_file_size(size: usize) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = size as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{:.0} {}", size, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("main.rs"), Some("rust".to_string()));
        assert_eq!(detect_language("app.js"), Some("javascript".to_string()));
        assert_eq!(detect_language("component.tsx"), Some("typescript".to_string()));
        assert_eq!(detect_language("script.py"), Some("python".to_string()));
        assert_eq!(detect_language("unknown.xyz"), None);
    }

    #[test]
    fn test_should_index_file() {
        assert!(should_index_file(Path::new("src/main.rs")));
        assert!(should_index_file(Path::new("README.md")));
        assert!(!should_index_file(Path::new("node_modules/package/index.js")));
        assert!(!should_index_file(Path::new(".git/config")));
        assert!(should_index_file(Path::new("package.json")));
    }

    #[test]
    fn test_is_sensitive_file() {
        assert!(is_sensitive_file(Path::new(".env")));
        assert!(is_sensitive_file(Path::new("config/secrets.json")));
        assert!(is_sensitive_file(Path::new("private_key.pem")));
        assert!(!is_sensitive_file(Path::new("src/main.rs")));
        assert!(!is_sensitive_file(Path::new("public/logo.png")));
    }

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1536), "1.5 KB");
        assert_eq!(format_file_size(1048576), "1.0 MB");
    }
}
