//! Server types for HTTP API
//!
//! Contains request/response types and SSE events.
//!
//! API Version: 2025.12.2 (breaking change - structured events)

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::tools::DiffInfo;

/// API version for capability detection
pub const API_VERSION: &str = "2025.12.2";

/// Tool categories for filtering in UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    File,
    Shell,
    Memory,
    Web,
    Git,
    Mira,
    Other,
}

impl std::fmt::Display for ToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCategory::File => write!(f, "file"),
            ToolCategory::Shell => write!(f, "shell"),
            ToolCategory::Memory => write!(f, "memory"),
            ToolCategory::Web => write!(f, "web"),
            ToolCategory::Git => write!(f, "git"),
            ToolCategory::Mira => write!(f, "mira"),
            ToolCategory::Other => write!(f, "other"),
        }
    }
}

// ============================================================================
// SSE Event Types
// ============================================================================

/// Events sent to the frontend via SSE
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)] // Variants for future rich streaming UI
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

    /// Streaming reasoning content from DeepSeek Reasoner
    #[serde(rename = "reasoning_delta")]
    ReasoningDelta { delta: String },

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
        /// Parent message ID for correlation
        message_id: String,
        /// Monotonic sequence number for ordering
        seq: u64,
        /// Timestamp in milliseconds
        ts_ms: u64,
        /// Human-readable summary (e.g., "Reading src/main.rs")
        summary: String,
        /// Tool category for filtering
        category: ToolCategory,
    },

    /// Tool result (may include diff for file operations)
    #[serde(rename = "tool_call_result")]
    ToolCallResult {
        call_id: String,
        name: String,
        success: bool,
        /// Output preview (truncated if large)
        output: String,
        /// Execution duration in milliseconds
        duration_ms: u64,
        /// Whether output was truncated
        truncated: bool,
        /// Total output size in bytes
        total_bytes: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<DiffInfo>,
        /// Reference ID to fetch full output on demand
        #[serde(skip_serializing_if = "Option::is_none")]
        output_ref: Option<String>,
        /// Exit code (shell commands only)
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        /// Stderr output (shell commands only)
        #[serde(skip_serializing_if = "Option::is_none")]
        stderr: Option<String>,
    },

    /// Artifact created (file, diff, or large output stored)
    #[serde(rename = "artifact_created")]
    ArtifactCreated {
        artifact_id: String,
        /// Artifact kind: "file", "diff", "patch", "log"
        kind: String,
        /// Tool call that created this artifact
        #[serde(skip_serializing_if = "Option::is_none")]
        source_call_id: Option<String>,
        /// Display title
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// File path if applicable
        #[serde(skip_serializing_if = "Option::is_none")]
        file_path: Option<String>,
        /// Programming language for syntax highlighting
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        /// Preview content
        preview: String,
        /// Total size in bytes
        total_bytes: usize,
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
#[allow(dead_code)] // reasoning_effort for future OpenAI o1/o3 reasoning control
pub struct ChatRequest {
    pub message: String,
    pub project_path: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
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
        /// Human-readable summary
        summary: String,
        /// Tool category for filtering
        category: ToolCategory,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<ToolCallResultData>,
    },
}

/// Tool call result for message persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResultData {
    pub success: bool,
    /// Output preview (truncated if large)
    pub output: String,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Whether output was truncated
    pub truncated: bool,
    /// Total output size in bytes
    pub total_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<DiffInfo>,
    /// Reference ID to fetch full output on demand
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_ref: Option<String>,
    /// Exit code (shell commands only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Stderr output (shell commands only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
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
