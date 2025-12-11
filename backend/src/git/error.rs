// backend/src/git/error.rs
// Error types for git operations

use thiserror::Error;

/// Git operation error type
#[derive(Error, Debug)]
pub enum GitError {
    #[error("Git error: {0}")]
    Git(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Join error: {0}")]
    JoinError(String),
}

impl GitError {
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn git(msg: impl Into<String>) -> Self {
        Self::Git(msg.into())
    }

    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }
}

/// Git operation result type
pub type GitResult<T> = Result<T, GitError>;

/// Trait for converting errors to git errors
pub trait IntoGitError {
    fn into_git_error(self) -> GitError;
}

/// Extension trait to add into_git_error method to Result types
pub trait IntoGitErrorResult<T> {
    fn into_git_error(self, context: &str) -> Result<T, GitError>;
}

impl<T, E: std::fmt::Display> IntoGitErrorResult<T> for Result<T, E> {
    fn into_git_error(self, context: &str) -> Result<T, GitError> {
        self.map_err(|e| GitError::Internal(format!("{}: {}", context, e)))
    }
}

impl IntoGitError for git2::Error {
    fn into_git_error(self) -> GitError {
        GitError::Git(self.message().to_string())
    }
}

impl IntoGitError for std::io::Error {
    fn into_git_error(self) -> GitError {
        GitError::Io(self)
    }
}

impl IntoGitError for anyhow::Error {
    fn into_git_error(self) -> GitError {
        GitError::Internal(self.to_string())
    }
}

impl IntoGitError for tokio::task::JoinError {
    fn into_git_error(self) -> GitError {
        GitError::JoinError(self.to_string())
    }
}
