// src/api/error.rs
// Centralized error handling for HTTP API responses
// Eliminates ~20+ instances of duplicated error handling across the codebase

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::fmt;
use tracing::error;

/// Standard API error response format
#[derive(Debug)]
pub struct ApiError {
    pub message: String,
    pub status_code: StatusCode,
    pub error_code: Option<String>,
}

impl ApiError {
    /// Create a new internal server error
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
            error_code: Some("INTERNAL_ERROR".to_string()),
        }
    }

    /// Create a new bad request error
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: StatusCode::BAD_REQUEST,
            error_code: Some("BAD_REQUEST".to_string()),
        }
    }

    /// Create a new not found error
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: StatusCode::NOT_FOUND,
            error_code: Some("NOT_FOUND".to_string()),
        }
    }

    /// Create a new unauthorized error
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: StatusCode::UNAUTHORIZED,
            error_code: Some("UNAUTHORIZED".to_string()),
        }
    }

    /// Create a new forbidden error
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: StatusCode::FORBIDDEN,
            error_code: Some("FORBIDDEN".to_string()),
        }
    }

    /// Create a new conflict error
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: StatusCode::CONFLICT,
            error_code: Some("CONFLICT".to_string()),
        }
    }

    /// Create a new unprocessable entity error
    pub fn unprocessable_entity(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: StatusCode::UNPROCESSABLE_ENTITY,
            error_code: Some("UNPROCESSABLE_ENTITY".to_string()),
        }
    }

    /// Create a new custom error with specific status code
    pub fn custom(status_code: StatusCode, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code,
            error_code: None,
        }
    }
}

// CRITICAL: Implement Display trait for std::error::Error
impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

// CRITICAL: Implement std::error::Error trait so anyhow can convert from it
impl std::error::Error for ApiError {}

// NOTE: anyhow automatically provides From<ApiError> for anyhow::Error
// since ApiError implements std::error::Error + Send + Sync + 'static

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response_json = json!({
            "error": true,
            "message": self.message,
            "status": self.status_code.as_u16()
        });

        if let Some(error_code) = self.error_code {
            response_json["error_code"] = json!(error_code);
        }

        (self.status_code, Json(response_json)).into_response()
    }
}

/// Result type alias for API operations
pub type ApiResult<T> = Result<T, ApiError>;

/// Macro for logging and creating internal server errors
/// Replaces the pattern: eprintln!("Failed to..."); (StatusCode::INTERNAL_SERVER_ERROR, "Failed to...").into_response()
#[macro_export]
macro_rules! internal_error {
    ($msg:expr) => {
        {
            tracing::error!($msg);
            $crate::api::error::ApiError::internal($msg)
        }
    };
    ($msg:expr, $($arg:tt)*) => {
        {
            let formatted_msg = format!($msg, $($arg)*);
            tracing::error!("{}", formatted_msg);
            $crate::api::error::ApiError::internal(formatted_msg)
        }
    };
}

/// Macro for logging and creating internal server errors from Result<T, E>
/// Replaces the pattern: .map_err(|e| { eprintln!("Failed to..."); ... })
#[macro_export]
macro_rules! map_internal_error {
    ($result:expr, $msg:expr) => {
        $result.map_err(|e| {
            let error_msg = format!("{}: {:?}", $msg, e);
            tracing::error!("{}", error_msg);
            $crate::api::error::ApiError::internal($msg)
        })
    };
}

/// Extension trait for converting common error types to ApiError
pub trait IntoApiError<T> {
    fn into_api_error(self, message: &str) -> Result<T, ApiError>;
    fn into_internal_error(self, message: &str) -> Result<T, ApiError>;
}

impl<T, E> IntoApiError<T> for Result<T, E>
where
    E: std::fmt::Debug,
{
    fn into_api_error(self, message: &str) -> Result<T, ApiError> {
        self.map_err(|e| {
            error!("{}: {:?}", message, e);
            ApiError::internal(message)
        })
    }

    fn into_internal_error(self, message: &str) -> Result<T, ApiError> {
        self.into_api_error(message)
    }
}

/// Extension trait for Option<T> to create ApiError for None cases
pub trait IntoApiErrorOption<T> {
    fn ok_or_not_found(self, message: &str) -> Result<T, ApiError>;
    fn ok_or_bad_request(self, message: &str) -> Result<T, ApiError>;
}

impl<T> IntoApiErrorOption<T> for Option<T> {
    fn ok_or_not_found(self, message: &str) -> Result<T, ApiError> {
        self.ok_or_else(|| ApiError::not_found(message))
    }

    fn ok_or_bad_request(self, message: &str) -> Result<T, ApiError> {
        self.ok_or_else(|| ApiError::bad_request(message))
    }
}

/// Helper function for database operation errors
pub fn db_error(operation: &str, error: impl std::fmt::Debug) -> ApiError {
    let message = format!("Database error during {operation}");
    error!("{}: {:?}", message, error);
    ApiError::internal(message)
}

/// Helper function for file system operation errors
pub fn fs_error(operation: &str, error: impl std::fmt::Debug) -> ApiError {
    let message = format!("File system error during {operation}");
    error!("{}: {:?}", message, error);
    ApiError::internal(message)
}

/// Helper function for git operation errors
pub fn git_error(operation: &str, error: impl std::fmt::Debug) -> ApiError {
    let message = format!("Git operation failed: {operation}");
    error!("{}: {:?}", message, error);
    ApiError::internal(message)
}

/// Helper function for serialization/deserialization errors
pub fn serde_error(operation: &str, error: impl std::fmt::Debug) -> ApiError {
    let message = format!("Serialization error during {operation}");
    error!("{}: {:?}", message, error);
    ApiError::bad_request(message)
}

/// Helper function for validation errors
pub fn validation_error(field: &str, reason: &str) -> ApiError {
    let message = format!("Validation failed for {field}: {reason}");
    ApiError::bad_request(message)
}

/// Helper function for missing parameter errors
pub fn missing_param_error(param_name: &str) -> ApiError {
    ApiError::bad_request(format!("Missing required parameter: {param_name}"))
}

/// Helper function for invalid parameter errors
pub fn invalid_param_error(param_name: &str, reason: &str) -> ApiError {
    ApiError::bad_request(format!("Invalid parameter '{param_name}': {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn test_api_error_creation() {
        let error = ApiError::internal("Test error");
        assert_eq!(error.status_code, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.message, "Test error");
    }

    #[test]
    fn test_into_api_error_extension() {
        let result: Result<i32, &str> = Err("test error");
        let api_result = result.into_api_error("Operation failed");
        
        assert!(api_result.is_err());
        let error = api_result.unwrap_err();
        assert_eq!(error.status_code, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.message, "Operation failed");
    }

    #[test]
    fn test_option_extensions() {
        let none_value: Option<i32> = None;
        let result = none_value.ok_or_not_found("Item not found");
        
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.status_code, StatusCode::NOT_FOUND);
        assert_eq!(error.message, "Item not found");
    }

    #[test]
    fn test_helper_functions() {
        let error = validation_error("email", "Invalid format");
        assert_eq!(error.status_code, StatusCode::BAD_REQUEST);
        assert!(error.message.contains("email"));
        assert!(error.message.contains("Invalid format"));
    }
}
