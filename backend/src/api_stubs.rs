// src/api_stubs.rs
// API error types - minimal stubs for git module compatibility

use thiserror::Error;

/// API Error type
#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Git error: {0}")]
    Git(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Join error: {0}")]
    JoinError(String),
}

impl ApiError {
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

/// API Result type
pub type ApiResult<T> = Result<T, ApiError>;

/// Trait for converting errors to API errors
pub trait IntoApiError {
    fn into_api_error(self) -> ApiError;
}

/// Extension trait to add into_api_error method to Result types
pub trait IntoApiErrorResult<T> {
    fn into_api_error(self, context: &str) -> Result<T, ApiError>;
}

impl<T, E: std::fmt::Display> IntoApiErrorResult<T> for Result<T, E> {
    fn into_api_error(self, context: &str) -> Result<T, ApiError> {
        self.map_err(|e| ApiError::Internal(format!("{}: {}", context, e)))
    }
}

impl IntoApiError for git2::Error {
    fn into_api_error(self) -> ApiError {
        ApiError::Git(self.message().to_string())
    }
}

impl IntoApiError for std::io::Error {
    fn into_api_error(self) -> ApiError {
        ApiError::Io(self)
    }
}

impl IntoApiError for anyhow::Error {
    fn into_api_error(self) -> ApiError {
        ApiError::Internal(self.to_string())
    }
}

impl IntoApiError for tokio::task::JoinError {
    fn into_api_error(self) -> ApiError {
        ApiError::JoinError(self.to_string())
    }
}
