//! Core error types
//!
//! Unified error handling for all core operations.
//! Wrappers (MCP, Chat) convert these to their protocol-specific formats.

use std::path::PathBuf;
use thiserror::Error;

/// Result type for core operations
pub type CoreResult<T> = Result<T, CoreError>;

/// Unified error type for core operations
#[derive(Debug, Error)]
pub enum CoreError {
    // File operations
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Permission denied: {0}")]
    PermissionDenied(PathBuf),

    #[error("Path is a directory: {0}")]
    IsDirectory(PathBuf),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("File too large: {path} ({size} bytes, max {max} bytes)")]
    FileTooLarge { path: PathBuf, size: u64, max: u64 },

    #[error("Failed to read file {0}: {1}")]
    FileRead(PathBuf, String),

    #[error("Failed to write file {0}: {1}")]
    FileWrite(PathBuf, String),

    // Edit operations
    #[error("String not found in file: {0}")]
    StringNotFound(String),

    #[error("String not unique: found {count} occurrences (use replace_all=true)")]
    StringNotUnique { count: usize },

    #[error("Edit target not found in {0}: {1}")]
    EditNotFound(String, String),

    #[error("Edit ambiguous in {0}: found {1} times (use replace_all=true)")]
    EditAmbiguous(String, usize),

    // Glob/Grep operations
    #[error("Invalid glob pattern: {0}")]
    GlobPattern(String),

    #[error("Invalid regex: {0}")]
    RegexInvalid(String),

    // Shell operations
    #[error("Command failed with exit code {code}: {stderr}")]
    CommandFailed { code: i32, stderr: String },

    #[error("Command timed out after {seconds}s")]
    CommandTimeout { seconds: u64 },

    #[error("Shell execution failed for '{0}': {1}")]
    ShellExec(String, String),

    #[error("Shell command '{0}' timed out after {1}s")]
    ShellTimeout(String, u64),

    // Web operations
    #[error("Failed to fetch {0}: {1}")]
    WebFetch(String, String),

    // Database operations
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Database not available")]
    DatabaseUnavailable,

    // Semantic search
    #[error("Semantic search not available")]
    SemanticUnavailable,

    #[error("Embedding failed: {0}")]
    EmbeddingFailed(String),

    // Network operations
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    // Git operations
    #[error("Not a git repository")]
    NotGitRepo,

    #[error("Git error: {0}")]
    Git(String),

    // Validation
    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Not found: {entity} with id {id}")]
    NotFound { entity: &'static str, id: String },

    // Provider/API errors
    #[error("Provider error ({provider}): {message}")]
    Provider { provider: String, message: String },

    #[error("API key not configured: {0}")]
    ApiKeyMissing(String),

    // Generic
    #[error("Operation cancelled")]
    Cancelled,

    #[error("Internal error: {0}")]
    Internal(String),

    // JSON parsing
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    // IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl CoreError {
    /// Create a provider error
    pub fn provider(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Provider {
            provider: provider.into(),
            message: message.into(),
        }
    }

    /// Create a not found error
    pub fn not_found(entity: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound {
            entity,
            id: id.into(),
        }
    }

    /// Check if this is a "not found" type error
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            CoreError::FileNotFound(_) | CoreError::NotFound { .. } | CoreError::StringNotFound(_)
        )
    }

    /// Check if this is a permission/auth error
    pub fn is_permission_error(&self) -> bool {
        matches!(
            self,
            CoreError::PermissionDenied(_) | CoreError::ApiKeyMissing(_)
        )
    }
}

// Convert to user-friendly string for tool output
impl CoreError {
    /// Format error for tool output (user-facing)
    pub fn to_tool_output(&self) -> String {
        format!("Error: {}", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = CoreError::FileNotFound(PathBuf::from("/foo/bar.txt"));
        assert_eq!(err.to_string(), "File not found: /foo/bar.txt");

        let err = CoreError::StringNotUnique { count: 3 };
        assert!(err.to_string().contains("3 occurrences"));
    }

    #[test]
    fn test_error_categories() {
        assert!(CoreError::FileNotFound(PathBuf::new()).is_not_found());
        assert!(CoreError::not_found("task", "abc123").is_not_found());
        assert!(CoreError::ApiKeyMissing("OPENAI".into()).is_permission_error());
    }
}
