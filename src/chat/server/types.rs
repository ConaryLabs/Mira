//! Server types for HTTP API
//!
//! Contains request/response types and SSE events.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::tools::DiffInfo;

// ============================================================================
// SSE Event Types
// ============================================================================

/// Events sent to the frontend via SSE
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ChatEvent {
    // === Message Boundaries ===
    /// Start of a new message
    #[serde(rename = "message_start")]
    MessageStart { message_id: String },

    /// End of message
    #[serde(rename = "message_end")]
    MessageEnd { message_id: String },

    // === Text Streaming ===
    /// Streaming text from assistant
    #[serde(rename = "text_delta")]
    TextDelta { delta: String },

    // === Code Block Streaming ===
    /// Code block started (detected ```)
    #[serde(rename = "code_block_start")]
    CodeBlockStart {
        id: String,
        language: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },

    /// Code block content delta
    #[serde(rename = "code_block_delta")]
    CodeBlockDelta { id: String, delta: String },

    /// Code block ended (detected closing ```)
    #[serde(rename = "code_block_end")]
    CodeBlockEnd { id: String },

    // === Council (multi-model) ===
    /// Council response from multiple models (GPT 5.2, Opus 4.5, Gemini 3 Pro)
    #[serde(rename = "council")]
    Council {
        #[serde(skip_serializing_if = "Option::is_none")]
        gpt: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        opus: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        gemini: Option<String>,
    },

    // === Tool Execution ===
    /// Tool call started - show in UI immediately
    #[serde(rename = "tool_call_start")]
    ToolCallStart {
        call_id: String,
        name: String,
        arguments: Value,
    },

    /// Tool result (may include diff for file operations)
    #[serde(rename = "tool_call_result")]
    ToolCallResult {
        call_id: String,
        name: String,
        success: bool,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<DiffInfo>,
    },

    // === Metadata ===
    /// Reasoning summary (when effort > none)
    #[serde(rename = "reasoning")]
    Reasoning {
        effort: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    /// Token usage at end
    #[serde(rename = "usage")]
    Usage {
        input_tokens: u32,
        output_tokens: u32,
        reasoning_tokens: u32,
        cached_tokens: u32,
    },

    /// Chain info for debugging (response_id linkage)
    #[serde(rename = "chain")]
    Chain {
        response_id: Option<String>,
        previous_response_id: Option<String>,
    },

    /// Stream complete
    #[serde(rename = "done")]
    Done,

    /// Error
    #[serde(rename = "error")]
    Error { message: String },
}

// ============================================================================
// Request/Response Types
// ============================================================================

/// Chat request from frontend
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub project_path: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

/// Message in the endless chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub blocks: Vec<MessageBlock>,
    pub created_at: i64,
}

/// Message with optional usage info (for API response)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageWithUsage {
    pub id: String,
    pub role: String,
    pub blocks: Vec<MessageBlock>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
}

/// Token usage info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub reasoning_tokens: u32,
    pub cached_tokens: u32,
}

/// Block within a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessageBlock {
    #[serde(rename = "text")]
    Text { content: String },

    #[serde(rename = "code_block")]
    CodeBlock {
        language: String,
        code: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },

    #[serde(rename = "council")]
    Council {
        #[serde(skip_serializing_if = "Option::is_none")]
        gpt: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        opus: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        gemini: Option<String>,
    },

    #[serde(rename = "tool_call")]
    ToolCall {
        call_id: String,
        name: String,
        arguments: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<ToolCallResult>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub success: bool,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<DiffInfo>,
}

/// Pagination query params
#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    #[serde(default = "default_limit")]
    pub limit: i32,
    pub before: Option<i64>, // created_at timestamp for cursor pagination
}

fn default_limit() -> i32 {
    50
}

/// Sync chat response (for Claude-to-Mira communication)
#[derive(Debug, Serialize)]
pub struct SyncChatResponse {
    pub request_id: String,
    pub timestamp: i64,
    pub role: String,
    pub content: String,
    pub blocks: Vec<MessageBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// Chain status: "NEW" if no previous_response_id, otherwise "…" + last 8 chars
    pub chain: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Structured error response for sync endpoint
#[derive(Debug, Serialize)]
pub struct SyncErrorResponse {
    pub request_id: String,
    pub timestamp: i64,
    pub success: bool,
    pub error: String,
}
