// src/tools/response.rs
// Helper functions for MCP tool responses

use rmcp::{ErrorData as McpError, model::{CallToolResult, Content}};
use serde::Serialize;

/// Convert an anyhow::Result to McpError
pub fn to_mcp_err(e: anyhow::Error) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

/// Create a success response with JSON content
pub fn json_response<T: Serialize>(result: T) -> CallToolResult {
    CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&result).unwrap()
    )])
}

/// Create a success response with plain text
pub fn text_response(message: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(message.into())])
}

/// Create a response for a Vec result - returns message if empty, JSON if not
pub fn vec_response<T: Serialize>(result: Vec<T>, empty_msg: impl Into<String>) -> CallToolResult {
    if result.is_empty() {
        text_response(empty_msg)
    } else {
        json_response(result)
    }
}

/// Create a response for an Option result - returns message if None, JSON if Some
pub fn option_response<T: Serialize>(result: Option<T>, none_msg: impl Into<String>) -> CallToolResult {
    match result {
        Some(r) => json_response(r),
        None => text_response(none_msg),
    }
}
