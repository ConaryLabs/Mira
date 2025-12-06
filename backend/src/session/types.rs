// backend/src/session/types.rs
// Type definitions for the dual-session architecture (Voice + Codex)

use serde::{Deserialize, Serialize};

/// Session type determines memory strategy and model routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    /// Eternal/rolling session with relationship continuity
    /// Uses: GPT-5.1 Voice tier, rolling summaries, semantic search
    #[default]
    Voice,

    /// Discrete task-scoped session for code work
    /// Uses: GPT-5.1-Codex-Max, native compaction, background execution
    Codex,
}

impl SessionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionType::Voice => "voice",
            SessionType::Codex => "codex",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "voice" => Some(SessionType::Voice),
            "codex" => Some(SessionType::Codex),
            _ => None,
        }
    }
}

/// Status of a Codex session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexStatus {
    /// Codex session is actively running
    Running,

    /// Codex session completed successfully
    Completed,

    /// Codex session failed with error
    Failed,

    /// Codex session was cancelled by user
    Cancelled,
}

impl CodexStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CodexStatus::Running => "running",
            CodexStatus::Completed => "completed",
            CodexStatus::Failed => "failed",
            CodexStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "running" => Some(CodexStatus::Running),
            "completed" => Some(CodexStatus::Completed),
            "failed" => Some(CodexStatus::Failed),
            "cancelled" => Some(CodexStatus::Cancelled),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            CodexStatus::Completed | CodexStatus::Failed | CodexStatus::Cancelled
        )
    }
}

/// Trigger that caused a Codex session to spawn
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum CodexSpawnTrigger {
    /// Router detected code-heavy task automatically
    RouterDetection {
        /// Confidence score (0.0-1.0)
        confidence: f32,
        /// What patterns triggered detection
        detected_patterns: Vec<String>,
    },

    /// User explicitly requested background work
    UserRequest {
        /// The command or phrase used
        trigger_phrase: Option<String>,
    },

    /// Task exceeded complexity threshold
    ComplexTask {
        /// Estimated tokens for the task
        estimated_tokens: i64,
        /// Number of files involved
        file_count: usize,
        /// Operation type that triggered
        operation_kind: Option<String>,
    },
}

impl CodexSpawnTrigger {
    pub fn trigger_type(&self) -> &'static str {
        match self {
            CodexSpawnTrigger::RouterDetection { .. } => "router_detection",
            CodexSpawnTrigger::UserRequest { .. } => "user_request",
            CodexSpawnTrigger::ComplexTask { .. } => "complex_task",
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::json!({}))
    }
}

/// Information about an active or completed Codex session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSessionInfo {
    /// Codex session ID
    pub id: String,

    /// Parent Voice session ID
    pub parent_voice_session_id: String,

    /// Current status
    pub status: CodexStatus,

    /// Brief description of the task
    pub task_description: String,

    /// When the session started (Unix timestamp)
    pub started_at: i64,

    /// When the session completed (Unix timestamp, None if still running)
    pub completed_at: Option<i64>,

    /// Progress percentage (0-100), if available
    pub progress_percent: Option<u8>,

    /// Current activity description
    pub current_activity: Option<String>,

    /// Total tokens used so far
    pub tokens_used: i64,

    /// Estimated cost so far
    pub cost_usd: f64,

    /// Number of times compaction was triggered
    pub compaction_count: u32,
}

/// Type of injection from Codex to Voice session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionType {
    /// Codex session completed successfully
    CodexCompletion,

    /// Progress update during long-running task
    CodexProgress,

    /// Codex session encountered an error
    CodexError,
}

impl InjectionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            InjectionType::CodexCompletion => "codex_completion",
            InjectionType::CodexProgress => "codex_progress",
            InjectionType::CodexError => "codex_error",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "codex_completion" => Some(InjectionType::CodexCompletion),
            "codex_progress" => Some(InjectionType::CodexProgress),
            "codex_error" => Some(InjectionType::CodexError),
            _ => None,
        }
    }
}

/// An injection record from Codex to Voice session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInjection {
    pub id: i64,
    pub target_session_id: String,
    pub source_session_id: String,
    pub injection_type: InjectionType,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
    pub injected_at: i64,
    pub acknowledged: bool,
    pub acknowledged_at: Option<i64>,
    pub sequence_num: i32,
}

/// Metadata for a Codex completion injection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexCompletionMetadata {
    /// Files that were created or modified
    pub files_changed: Vec<String>,

    /// Total duration in seconds
    pub duration_seconds: i64,

    /// Total tokens used (input + output)
    pub tokens_total: i64,

    /// Total cost in USD
    pub cost_usd: f64,

    /// Number of tool calls made
    pub tool_calls_count: u32,

    /// Number of compaction events
    pub compaction_count: u32,

    /// Key actions taken (brief summaries)
    pub key_actions: Vec<String>,
}

/// Link between Voice and Codex sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSessionLink {
    pub id: i64,
    pub voice_session_id: String,
    pub codex_session_id: String,
    pub spawn_trigger: String,
    pub spawn_confidence: Option<f32>,
    pub voice_context_summary: Option<String>,
    pub completion_summary: Option<String>,
    pub tokens_used_input: i64,
    pub tokens_used_output: i64,
    pub cost_usd: f64,
    pub compaction_count: i32,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_type_default() {
        let session_type = SessionType::default();
        assert_eq!(session_type, SessionType::Voice);
    }

    #[test]
    fn test_session_type_conversion() {
        assert_eq!(SessionType::Voice.as_str(), "voice");
        assert_eq!(SessionType::Codex.as_str(), "codex");
        assert_eq!(SessionType::from_str("voice"), Some(SessionType::Voice));
        assert_eq!(SessionType::from_str("CODEX"), Some(SessionType::Codex));
        assert_eq!(SessionType::from_str("invalid"), None);
    }

    #[test]
    fn test_codex_status_terminal() {
        assert!(!CodexStatus::Running.is_terminal());
        assert!(CodexStatus::Completed.is_terminal());
        assert!(CodexStatus::Failed.is_terminal());
        assert!(CodexStatus::Cancelled.is_terminal());
    }

    #[test]
    fn test_spawn_trigger_serialization() {
        let trigger = CodexSpawnTrigger::RouterDetection {
            confidence: 0.85,
            detected_patterns: vec!["implement".to_string(), "refactor".to_string()],
        };
        let json = trigger.to_json();
        assert_eq!(json["type"], "router_detection");
        // Compare as f64 with tolerance for f32 precision
        let confidence = json["confidence"].as_f64().unwrap();
        assert!((confidence - 0.85).abs() < 0.001);
    }
}
