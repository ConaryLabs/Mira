// mira-types: Shared types for Mira (native + WASM compatible)
// No native-only dependencies allowed here

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════
// DOMAIN TYPES
// ═══════════════════════════════════════

/// Project context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub id: i64,
    pub path: String,
    pub name: Option<String>,
}

/// Memory fact from semantic memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub id: i64,
    pub project_id: Option<i64>,
    pub key: Option<String>,
    pub content: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub created_at: String,
}

/// Code symbol extracted by indexer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub qualified_name: Option<String>,
    pub symbol_type: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
    pub visibility: Option<String>,
    pub documentation: Option<String>,
    pub is_test: bool,
    pub is_async: bool,
}

/// Task status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
        }
    }
}

/// Priority level
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Medium,
    High,
    Urgent,
}

impl Priority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Urgent => "urgent",
        }
    }
}

/// Goal status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Planning,
    InProgress,
    Blocked,
    Completed,
    Abandoned,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
        }
    }
}

/// Task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub project_id: Option<i64>,
    pub goal_id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub priority: Priority,
    pub created_at: String,
}

/// Goal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: i64,
    pub project_id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub status: GoalStatus,
    pub priority: Priority,
    pub progress_percent: i32,
    pub created_at: String,
}

/// Milestone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub id: i64,
    pub goal_id: i64,
    pub title: String,
    pub weight: i32,
    pub completed: bool,
    pub completed_at: Option<String>,
}

// ═══════════════════════════════════════
// AGENT COLLABORATION
// ═══════════════════════════════════════

/// Agent role in collaboration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Mira,
    Claude,
}

// ═══════════════════════════════════════
// WEBSOCKET EVENTS
// ═══════════════════════════════════════

/// Thinking phase for streaming
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingPhase {
    Analyzing,
    Planning,
    Executing,
    Reviewing,
}

/// File change type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    Created,
    Modified,
    Deleted,
}

/// Single diff line - enum for cleaner pattern matching
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum DiffLine {
    Context(String),
    Add(String),
    Remove(String),
}

/// Diff hunk
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

/// Unified diff representation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnifiedDiff {
    pub file_path: String,
    pub hunks: Vec<DiffHunk>,
}

/// WebSocket event for Ghost Mode streaming
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    // Agent reasoning stream
    Thinking {
        content: String,
        phase: ThinkingPhase,
    },

    // Tool execution
    ToolStart {
        tool_name: String,
        arguments: serde_json::Value,
        call_id: String,
    },
    ToolResult {
        tool_name: String,
        result: String,
        success: bool,
        call_id: String,
        duration_ms: u64,
    },

    // File operations
    DiffPreview {
        diff: UnifiedDiff,
    },
    FileChange {
        file_path: String,
        change_type: ChangeType,
    },

    // Terminal output
    TerminalOutput {
        instance_id: String,
        content: String,
        is_stderr: bool,
    },

    // Session events
    SessionUpdate {
        memories_added: usize,
        tools_called: usize,
    },

    // Errors
    Error {
        message: String,
        recoverable: bool,
    },

    // Connection
    Connected {
        session_id: String,
    },
    Ping,
    Pong,

    // ═══════════════════════════════════════
    // CLAUDE CODE EVENTS
    // ═══════════════════════════════════════

    /// Claude Code instance spawned
    ClaudeSpawned {
        instance_id: String,
        working_dir: String,
    },

    /// Claude Code instance stopped
    ClaudeStopped {
        instance_id: String,
    },

    // ═══════════════════════════════════════
    // AGENT COLLABORATION EVENTS
    // ═══════════════════════════════════════

    /// Agent-to-agent message
    AgentMessage {
        message_id: String,
        from: AgentRole,
        to: AgentRole,
        content: String,
        thread_id: String,
    },

    /// Agent-to-agent response
    AgentResponse {
        in_reply_to: String,
        from: AgentRole,
        content: String,
        complete: bool,
    },
}

/// Client-to-server WebSocket message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsCommand {
    /// Sync from a specific event ID (reconnection)
    Sync { last_event_id: Option<i64> },
    /// Cancel current operation
    Cancel,
    /// Ping for keepalive
    Ping,
}

// ═══════════════════════════════════════
// CHAT API TYPES
// ═══════════════════════════════════════

/// Chat message role
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Chat message for API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Chat request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(default)]
    pub history: Vec<ChatMessage>,
}

// ═══════════════════════════════════════
// API REQUEST/RESPONSE TYPES
// ═══════════════════════════════════════

/// Remember request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RememberRequest {
    pub content: String,
    pub key: Option<String>,
    pub fact_type: Option<String>,
    pub category: Option<String>,
    pub confidence: Option<f64>,
}

/// Recall request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallRequest {
    pub query: String,
    pub limit: Option<i64>,
    pub category: Option<String>,
    pub fact_type: Option<String>,
}

/// Recall response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResponse {
    pub memories: Vec<MemoryFact>,
}

/// Semantic code search request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSearchRequest {
    pub query: String,
    pub language: Option<String>,
    pub limit: Option<i64>,
}

/// Code search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSearchResult {
    pub file_path: String,
    pub line_number: u32,
    pub content: String,
    pub symbol_name: Option<String>,
    pub symbol_type: Option<String>,
    pub score: f32,
}

/// Code search response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSearchResponse {
    pub results: Vec<CodeSearchResult>,
}

/// Index request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexRequest {
    pub path: Option<String>,
}

/// Index stats response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub files: usize,
    pub symbols: usize,
    pub chunks: usize,
    #[serde(default)]
    pub errors: usize,
}

/// Generic API response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
        }
    }
}
