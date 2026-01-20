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