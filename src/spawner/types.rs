//! Types for Claude Code spawner
//!
//! Defines the core data structures for managing spawned Claude Code sessions.

use serde::{Deserialize, Serialize};

// ============================================================================
// Session Types
// ============================================================================

/// Status of a Claude Code session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Session created but not yet spawned
    Pending,
    /// Process is starting up
    Starting,
    /// Process is running and active
    Running,
    /// Waiting for user input (question pending)
    Paused,
    /// Session completed successfully
    Completed,
    /// Session failed with error
    Failed,
}

#[allow(dead_code)]
impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "starting" => Some(Self::Starting),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

/// Configuration for spawning a Claude Code session
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    /// Project root directory
    pub project_path: String,
    /// Initial prompt to send
    pub initial_prompt: String,
    /// Optional system prompt to append
    pub system_prompt: Option<String>,
    /// Pre-computed context snapshot
    pub context_snapshot: Option<ContextSnapshot>,
    /// Maximum budget in USD (default: $5.00)
    pub max_budget_usd: Option<f64>,
    /// Allowed tools (None = all tools allowed)
    pub allowed_tools: Option<Vec<String>>,
    /// Session ID to use (auto-generated if None)
    pub session_id: Option<String>,
    /// Instruction ID this session is executing (for completion tracking)
    pub instruction_id: Option<String>,
}

#[allow(dead_code)]
impl SpawnConfig {
    pub fn new(project_path: impl Into<String>, initial_prompt: impl Into<String>) -> Self {
        Self {
            project_path: project_path.into(),
            initial_prompt: initial_prompt.into(),
            system_prompt: None,
            context_snapshot: None,
            max_budget_usd: Some(5.0),
            allowed_tools: None,
            session_id: None,
            instruction_id: None,
        }
    }

    pub fn with_instruction(mut self, instruction_id: impl Into<String>) -> Self {
        self.instruction_id = Some(instruction_id.into());
        self
    }

    pub fn with_context(mut self, snapshot: ContextSnapshot) -> Self {
        self.context_snapshot = Some(snapshot);
        self
    }

    pub fn with_budget(mut self, usd: f64) -> Self {
        self.max_budget_usd = Some(usd);
        self
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = Some(tools);
        self
    }
}

// ============================================================================
// Context Handoff
// ============================================================================

/// Pre-computed context from Mira to hand off to Claude Code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnapshot {
    /// High-level overview of the task batch
    pub task_overview: String,
    /// Relevant decisions from memory
    pub relevant_decisions: Vec<String>,
    /// Active goals with progress
    pub active_goals: Vec<GoalSummary>,
    /// Corrections to apply (don't repeat mistakes)
    pub corrections: Vec<CorrectionSummary>,
    /// Key files to focus on
    pub key_files: Vec<String>,
    /// Anti-patterns to avoid
    pub anti_patterns: Vec<String>,
}

impl ContextSnapshot {
    /// Format as a system prompt section
    pub fn to_system_prompt(&self) -> String {
        let mut sections = Vec::new();

        sections.push("## Strategic Context from Mira\n".to_string());

        // Task overview
        sections.push(format!("### Task Overview\n{}\n", self.task_overview));

        // Active goals
        if !self.active_goals.is_empty() {
            let goals: String = self
                .active_goals
                .iter()
                .map(|g| format!("- {} ({}% complete)", g.title, g.progress_percent))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("### Active Goals\n{}\n", goals));
        }

        // Key decisions
        if !self.relevant_decisions.is_empty() {
            let decisions = self
                .relevant_decisions
                .iter()
                .map(|d| format!("- {}", d))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("### Key Decisions to Honor\n{}\n", decisions));
        }

        // Corrections
        if !self.corrections.is_empty() {
            let corrections: String = self
                .corrections
                .iter()
                .map(|c| format!("- {} -> {}", c.what_was_wrong, c.what_is_right))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("### Corrections to Apply\n{}\n", corrections));
        }

        // Key files
        if !self.key_files.is_empty() {
            sections.push(format!(
                "### Files to Focus On\n{}\n",
                self.key_files.join("\n")
            ));
        }

        // Anti-patterns
        if !self.anti_patterns.is_empty() {
            let patterns = self
                .anti_patterns
                .iter()
                .map(|p| format!("- Avoid: {}", p))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("### Anti-Patterns\n{}\n", patterns));
        }

        sections.join("\n")
    }
}

/// Summary of an active goal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub progress_percent: i32,
}

/// Summary of a correction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionSummary {
    pub what_was_wrong: String,
    pub what_is_right: String,
}

// ============================================================================
// Stream Events (Claude Code output)
// ============================================================================

/// Events parsed from Claude Code's stream-json output
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// System message about session
    System {
        subtype: String,
        #[serde(flatten)]
        data: serde_json::Value,
    },

    /// Assistant text message
    Assistant {
        message: AssistantMessage,
        /// Error type if this was an error response
        #[serde(default)]
        error: Option<String>,
        /// Session ID
        #[serde(default)]
        session_id: Option<String>,
    },

    /// Tool being used
    ToolUse {
        name: String,
        id: String,
        input: serde_json::Value,
    },

    /// Result of tool execution
    ToolResult {
        id: String,
        content: String,
        is_error: Option<bool>,
    },

    /// User message with tool results (echoed back from Claude Code)
    User {
        message: UserMessage,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        uuid: Option<String>,
        /// Additional tool result metadata
        #[serde(default)]
        tool_use_result: Option<serde_json::Value>,
    },

    /// Error from Claude Code
    Error {
        error: StreamError,
    },

    /// Session completion
    Result {
        #[serde(flatten)]
        data: serde_json::Value,
    },
}

/// Assistant message content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    /// Content can be a string or array of content blocks
    #[serde(default)]
    pub content: serde_json::Value,
    pub stop_reason: Option<String>,
    pub id: Option<String>,
    pub model: Option<String>,
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    /// Capture any extra fields
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl AssistantMessage {
    /// Get content as string (handling both string and array formats)
    pub fn content_text(&self) -> Option<String> {
        match &self.content {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(arr) => {
                // Extract text from content blocks
                let texts: Vec<&str> = arr
                    .iter()
                    .filter_map(|block| {
                        if block.get("type")?.as_str()? == "text" {
                            block.get("text")?.as_str()
                        } else {
                            None
                        }
                    })
                    .collect();
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join(""))
                }
            }
            _ => None,
        }
    }
}

/// User message content (can be text or tool results)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub role: String,
    /// Content can be a string (user text) or array of tool results
    #[serde(default)]
    pub content: UserContent,
}

/// User message content - either text or tool results
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum UserContent {
    /// Plain text content (from injected messages)
    Text(String),
    /// Tool result content blocks
    ToolResults(Vec<ToolResultContent>),
    /// Empty/default
    #[default]
    Empty,
}

/// Tool result content block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultContent {
    pub tool_use_id: String,
    #[serde(rename = "type")]
    pub content_type: String,
    /// Tool result content (can be string or structured)
    #[serde(default)]
    pub content: serde_json::Value,
    #[serde(default)]
    pub is_error: Option<bool>,
}

impl UserMessage {
    /// Get a summary of the user message for logging
    pub fn summary(&self) -> String {
        match &self.content {
            UserContent::Text(s) => {
                let preview = if s.len() > 100 { format!("{}...", &s[..100]) } else { s.clone() };
                format!("text: {}", preview)
            }
            UserContent::ToolResults(results) => {
                let tool_ids: Vec<&str> = results.iter().map(|c| c.tool_use_id.as_str()).collect();
                format!("{} tool result(s): {}", tool_ids.len(), tool_ids.join(", "))
            }
            UserContent::Empty => "empty".to_string(),
        }
    }

    /// Check if this is a text message (injected instruction)
    pub fn is_text(&self) -> bool {
        matches!(&self.content, UserContent::Text(_))
    }

    /// Get the text content if this is a text message
    pub fn text(&self) -> Option<&str> {
        match &self.content {
            UserContent::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// Error details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamError {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
}

// ============================================================================
// Question Relay
// ============================================================================

/// Question that needs user input
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct PendingQuestion {
    pub id: String,
    pub session_id: String,
    pub question: String,
    pub options: Option<Vec<QuestionOption>>,
    pub context: Option<String>,
    pub status: QuestionStatus,
    pub created_at: i64,
}

/// Option for a question
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

/// Status of a pending question
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum QuestionStatus {
    Pending,
    Answered,
    Expired,
}

// ============================================================================
// Session Events (for SSE broadcast)
// ============================================================================

/// Events broadcast to Studio about session activity
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    /// Session spawned
    Started {
        session_id: String,
        project_path: String,
        initial_prompt: String,
    },

    /// Session status changed
    StatusChanged {
        session_id: String,
        status: SessionStatus,
        phase: Option<String>,
    },

    /// Output chunk from session
    Output {
        session_id: String,
        chunk_type: String,
        content: String,
    },

    /// Tool being used
    ToolCall {
        session_id: String,
        tool_name: String,
        tool_id: String,
        input_preview: String,
    },

    /// Question needs user answer
    QuestionPending {
        question_id: String,
        session_id: String,
        question: String,
        options: Option<Vec<QuestionOption>>,
    },

    /// Session ended
    Ended {
        session_id: String,
        status: SessionStatus,
        exit_code: Option<i32>,
        summary: Option<String>,
    },

    /// Heartbeat to keep SSE connection alive
    Heartbeat { ts: i64 },

    /// Raw internal SSE event from Claude Code (relayed from CLAUDE_CODE_SSE_PORT)
    RawInternalEvent {
        session_id: String,
        /// Event type from Claude Code internal SSE
        event_type: String,
        /// Raw JSON data from the event
        data: serde_json::Value,
        /// Timestamp when relayed
        ts: i64,
    },
}

// ============================================================================
// Session Details (for API responses)
// ============================================================================

/// Session details for list endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetails {
    pub session_id: String,
    pub status: SessionStatus,
    pub project_path: Option<String>,
    pub spawned_at: Option<i64>,
}

// ============================================================================
// Session Review
// ============================================================================

/// Review of a completed session by Mira
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SessionReview {
    pub session_id: String,
    pub status: ReviewStatus,
    pub summary: String,
    pub decisions_made: Vec<String>,
    pub files_changed: Vec<String>,
    pub feedback: Option<String>,
    pub follow_up_instructions: Vec<String>,
}

/// Status of session review
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum ReviewStatus {
    Approved,
    NeedsChanges,
    Failed,
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the spawner
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SpawnerConfig {
    /// Path to claude binary (default: "claude")
    pub claude_binary: String,
    /// Path to MCP config for spawned sessions
    pub mcp_config_path: Option<String>,
    /// Default budget per session in USD
    pub default_budget_usd: f64,
    /// Maximum concurrent sessions
    pub max_concurrent_sessions: usize,
    /// Whether to store full output for review
    pub store_full_output: bool,
}

impl Default for SpawnerConfig {
    fn default() -> Self {
        Self {
            claude_binary: "claude".to_string(),
            mcp_config_path: None,
            default_budget_usd: 5.0,
            max_concurrent_sessions: 3,
            store_full_output: true,
        }
    }
}

impl SpawnerConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(val) = std::env::var("MIRA_CLAUDE_BINARY") {
            config.claude_binary = val;
        }
        if let Ok(val) = std::env::var("MIRA_MCP_CONFIG") {
            config.mcp_config_path = Some(val);
        }
        if let Ok(val) = std::env::var("MIRA_DEFAULT_BUDGET") {
            if let Ok(budget) = val.parse() {
                config.default_budget_usd = budget;
            }
        }
        if let Ok(val) = std::env::var("MIRA_MAX_SESSIONS") {
            if let Ok(max) = val.parse() {
                config.max_concurrent_sessions = max;
            }
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_status_roundtrip() {
        for status in [
            SessionStatus::Pending,
            SessionStatus::Starting,
            SessionStatus::Running,
            SessionStatus::Paused,
            SessionStatus::Completed,
            SessionStatus::Failed,
        ] {
            let s = status.as_str();
            let parsed = SessionStatus::from_str(s);
            assert_eq!(parsed, Some(status));
        }
    }

    #[test]
    fn test_context_snapshot_format() {
        let snapshot = ContextSnapshot {
            task_overview: "Implement feature X".to_string(),
            relevant_decisions: vec!["Use async/await".to_string()],
            active_goals: vec![GoalSummary {
                id: "g1".to_string(),
                title: "Ship v1.0".to_string(),
                status: "in_progress".to_string(),
                progress_percent: 50,
            }],
            corrections: vec![CorrectionSummary {
                what_was_wrong: "Using unwrap()".to_string(),
                what_is_right: "Use expect() with message".to_string(),
            }],
            key_files: vec!["src/main.rs".to_string()],
            anti_patterns: vec!["Blocking in async context".to_string()],
        };

        let prompt = snapshot.to_system_prompt();
        assert!(prompt.contains("Implement feature X"));
        assert!(prompt.contains("Use async/await"));
        assert!(prompt.contains("Ship v1.0"));
        assert!(prompt.contains("50% complete"));
    }
}
