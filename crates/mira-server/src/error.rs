// crates/mira-server/src/error.rs
// Standardized error types for Mira

use thiserror::Error;

/// Main error type for the Mira library
#[derive(Error, Debug)]
pub enum MiraError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("project not set")]
    ProjectNotSet,

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("tree-sitter parsing error")]
    TreeSitter,

    #[error("embedding error: {0}")]
    Embedding(String),

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("task cancelled")]
    Cancelled,

    #[error("configuration error: {0}")]
    Config(String),

    #[error("unknown error: {0}")]
    Other(String),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

/// Convenience type alias for Result using MiraError
pub type Result<T> = std::result::Result<T, MiraError>;

impl MiraError {
    /// Convert to user-facing string for MCP tool boundaries
    pub fn to_user_string(&self) -> String {
        self.to_string()
    }
}

impl From<String> for MiraError {
    fn from(s: String) -> Self {
        MiraError::Other(s)
    }
}

impl From<tokio::task::JoinError> for MiraError {
    fn from(err: tokio::task::JoinError) -> Self {
        if err.is_cancelled() {
            MiraError::Cancelled
        } else {
            MiraError::Other(err.to_string())
        }
    }
}

impl From<MiraError> for String {
    fn from(err: MiraError) -> Self {
        err.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // MiraError construction tests
    // ============================================================================

    #[test]
    fn test_invalid_input_error() {
        let err = MiraError::InvalidInput("bad data".to_string());
        assert!(err.to_string().contains("invalid input"));
        assert!(err.to_string().contains("bad data"));
    }

    #[test]
    fn test_project_not_set_error() {
        let err = MiraError::ProjectNotSet;
        assert!(err.to_string().contains("project not set"));
    }

    #[test]
    fn test_tree_sitter_error() {
        let err = MiraError::TreeSitter;
        assert!(err.to_string().contains("tree-sitter"));
    }

    #[test]
    fn test_embedding_error() {
        let err = MiraError::Embedding("dimension mismatch".to_string());
        assert!(err.to_string().contains("embedding error"));
        assert!(err.to_string().contains("dimension mismatch"));
    }

    #[test]
    fn test_llm_error() {
        let err = MiraError::Llm("rate limited".to_string());
        assert!(err.to_string().contains("LLM error"));
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn test_cancelled_error() {
        let err = MiraError::Cancelled;
        assert!(err.to_string().contains("cancelled"));
    }

    #[test]
    fn test_config_error() {
        let err = MiraError::Config("missing key".to_string());
        assert!(err.to_string().contains("configuration error"));
        assert!(err.to_string().contains("missing key"));
    }

    #[test]
    fn test_other_error() {
        let err = MiraError::Other("something unexpected".to_string());
        assert!(err.to_string().contains("unknown error"));
        assert!(err.to_string().contains("something unexpected"));
    }

    // ============================================================================
    // to_user_string tests
    // ============================================================================

    #[test]
    fn test_to_user_string() {
        let err = MiraError::InvalidInput("test".to_string());
        assert_eq!(err.to_user_string(), err.to_string());
    }

    // ============================================================================
    // From implementations tests
    // ============================================================================

    #[test]
    fn test_from_string() {
        let err: MiraError = "some error".to_string().into();
        assert!(matches!(err, MiraError::Other(_)));
        assert!(err.to_string().contains("some error"));
    }

    #[test]
    fn test_into_string() {
        let err = MiraError::Llm("test".to_string());
        let s: String = err.into();
        assert!(s.contains("LLM error"));
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: MiraError = io_err.into();
        assert!(matches!(err, MiraError::Io(_)));
        assert!(err.to_string().contains("I/O error"));
    }

    #[test]
    fn test_from_json_error() {
        let json_err = serde_json::from_str::<i32>("not json").unwrap_err();
        let err: MiraError = json_err.into();
        assert!(matches!(err, MiraError::Json(_)));
        assert!(err.to_string().contains("JSON"));
    }

    // ============================================================================
    // Debug trait tests
    // ============================================================================

    #[test]
    fn test_debug_impl() {
        let err = MiraError::InvalidInput("debug test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("InvalidInput"));
    }

    // ============================================================================
    // Result type alias tests
    // ============================================================================

    #[test]
    fn test_result_ok() {
        let result: Result<i32> = Ok(42);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_result_err() {
        let result: Result<i32> = Err(MiraError::ProjectNotSet);
        assert!(result.is_err());
    }
}