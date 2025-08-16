// src/llm/responses/types.rs
// Updated for GPT-5 Responses API - August 15, 2025
// Changes:
// - Removed obsolete CreateRunRequest, RunResponse, RunStatus types
// - Added new types for GPT-5 Responses API
// - Added support for previous_response_id tracking

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// GPT-5 Responses API Types
// ============================================================================

/// Request structure for the Responses API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,  // For conversation continuity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    // GPT-5 specific parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// Message structure for input/output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,
}

/// Response format specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,  // "text", "json_object", "json_schema"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<Value>,
}

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,  // "function", "web_search_preview", "code_interpreter", "custom"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search_preview: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_interpreter: Option<CodeInterpreterConfig>,
}

/// Function definition for tool calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,  // JSON Schema
}

/// Code interpreter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeInterpreterConfig {
    pub container: ContainerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    #[serde(rename = "type")]
    pub container_type: String,  // "auto" or specific container type
}

/// Response from the Responses API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesResponse {
    pub id: String,  // Use this as previous_response_id in follow-up calls
    pub object: String,
    pub created: i64,
    pub model: String,
    pub output: Vec<OutputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
}

/// Output item in a response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputItem {
    #[serde(rename = "type")]
    pub output_type: String,  // "text", "function_call", "tool_call"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<Value>,
}

/// Usage information for token counting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i32>,  // GPT-5 specific
}

// ============================================================================
// Streaming Types
// ============================================================================

/// Streaming response chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<StreamingChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingChoice {
    pub index: i32,
    pub delta: StreamingDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,
}

// ============================================================================
// REMOVED: Obsolete Assistants API Types
// ============================================================================
// The following types have been removed as they are no longer used:
// - CreateRunRequest (used thread_id from old Assistants API)
// - RunResponse (old Assistants API response format)  
// - RunStatus (old Assistants API status tracking)
// 
// These were part of the deprecated Threads/Runs API that is being sunset in 2026.
// The new Responses API uses previous_response_id for conversation continuity
// instead of server-side thread management.
