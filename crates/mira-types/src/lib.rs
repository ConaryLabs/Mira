// crates/mira-types/src/lib.rs

//! Shared data contracts between the Mira native server and its clients.
//!
//! This crate provides the core domain model for:
//! - **Project context**: Mapping filesystem paths to database entities
//! - **Semantic memory**: Evidence-based facts with lifecycle and scoping
//! - **WebSocket protocol**: Real-time tool execution and agent collaboration
//!
//! These types are designed to work across native and WASM builds,
//! with no native-only dependencies allowed.

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════
// DOMAIN TYPES
// ═══════════════════════════════════════

/// Represents the connection between a local filesystem path and a Mira database entity.
///
/// This context is required for almost all operations (indexing, memory retrieval, chat).
/// It ensures that memories and preferences are scoped to the correct workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    /// The persistent database ID for this project. Use this for all foreign keys.
    pub id: i64,
    /// The absolute filesystem path to the project root. Used for file operations.
    pub path: String,
    /// Human-readable display name (e.g., directory name or parsed from package.json/Cargo.toml).
    pub name: Option<String>,
}

/// A semantic unit of knowledge derived from user interactions or code analysis.
///
/// # Lifecycle
///
/// 1. **Creation**: Created as `status: "candidate"` with initial confidence (capped at 0.5).
/// 2. **Reinforcement**: If the fact is recalled and useful in subsequent sessions,
///    `session_count` increments and `confidence` increases.
/// 3. **Verification**: High-confidence facts effectively become permanent knowledge.
///
/// # Scoping
///
/// Controls visibility via the `scope` field:
/// - `"project"` (default): Visible only within `project_id`.
/// - `"user"`: Visible to the specific `user_id` across all their projects.
/// - `"team"`: Visible to all members of `team_id`.
/// - `"global"`: Universal constants (rare).
///
/// # Fact Types
///
/// The `fact_type` field classifies the kind of knowledge:
/// - `"general"`: General observations or context
/// - `"preference"`: User preferences (coding style, tooling choices)
/// - `"decision"`: Architectural or design decisions with rationale
/// - `"context"`: Project-specific context (tech stack, conventions)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub id: i64,
    /// The project this fact originated from. Required unless scope is "user" or "global".
    pub project_id: Option<i64>,
    /// Optional structured key for key-value lookups (e.g., "preference:linter").
    pub key: Option<String>,
    /// The natural language fact (e.g., "The user prefers strictly typed Python code").
    pub content: String,
    /// Classification: "general", "preference", "decision", "context".
    pub fact_type: String,
    /// Broad grouping for filtering (e.g., "coding", "tooling", "architecture").
    pub category: Option<String>,
    /// Confidence score from 0.0 to 1.0. Higher values indicate verified knowledge.
    pub confidence: f64,
    /// ISO 8601 timestamp of when this fact was created.
    pub created_at: String,

    // Evidence-based memory fields
    /// Number of distinct sessions where this fact was recalled or reinforced.
    #[serde(default = "default_session_count")]
    pub session_count: i32,
    /// Session ID where this fact was first observed.
    #[serde(default)]
    pub first_session_id: Option<String>,
    /// Session ID where this fact was most recently reinforced.
    #[serde(default)]
    pub last_session_id: Option<String>,
    /// State of the fact: "candidate", "verified", or "rejected".
    #[serde(default = "default_status")]
    pub status: String,

    // Multi-user memory sharing fields
    /// Owner of the memory (for user-scoped memories).
    #[serde(default)]
    pub user_id: Option<String>,
    /// Visibility scope: "project", "user", "team", or "global".
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Team ID (required if scope is "team").
    #[serde(default)]
    pub team_id: Option<i64>,
}

fn default_session_count() -> i32 {
    1
}

fn default_status() -> String {
    "candidate".to_string()
}

fn default_scope() -> String {
    "project".to_string()
}

// ═══════════════════════════════════════
// AGENT COLLABORATION
// ═══════════════════════════════════════

/// Identifies the speaker in a chat or collaboration stream.
///
/// Used by clients to determine avatar/styling and attribute messages correctly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// The local Mira orchestrator/server.
    Mira,
    /// The remote LLM intelligence (Claude).
    Claude,
}

// ═══════════════════════════════════════
// WEBSOCKET EVENTS
// ═══════════════════════════════════════

/// Real-time events for the Model Context Protocol (MCP) and chat stream.
///
/// # Protocol Flow
///
/// ## Tool Execution
/// 1. Server emits [`WsEvent::ToolStart`] with a unique `call_id`.
/// 2. Tool executes (either on server or client).
/// 3. [`WsEvent::ToolResult`] is emitted with the matching `call_id` and output.
///
/// ## Chat Streaming
/// - [`WsEvent::AgentResponse`] streams chunks of text.
/// - `complete: true` signals the end of a turn.
///
/// # Correlation
///
/// `ToolStart` and `ToolResult` are correlated by `call_id`. Clients must track
/// pending tool calls and match results to their corresponding starts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    /// The agent has decided to call a tool.
    ToolStart {
        /// Name of the tool being invoked.
        tool_name: String,
        /// JSON arguments map passed to the tool.
        arguments: serde_json::Value,
        /// Unique correlation ID to match with [`WsEvent::ToolResult`].
        call_id: String,
    },
    /// The output of a tool execution.
    ToolResult {
        /// Name of the tool that was invoked.
        tool_name: String,
        /// The tool's output (typically JSON or plain text).
        result: String,
        /// Whether the tool executed successfully.
        success: bool,
        /// Must match the `call_id` from the corresponding [`WsEvent::ToolStart`].
        call_id: String,
        /// Execution time in milliseconds.
        duration_ms: u64,
    },
    /// A chat message or thought from an agent.
    AgentResponse {
        /// ID of the user message this is replying to.
        in_reply_to: String,
        /// Which agent sent this response.
        from: AgentRole,
        /// The message content (may be a partial chunk if streaming).
        content: String,
        /// If false, this is a partial stream chunk. If true, the message is complete.
        complete: bool,
    },
    /// Progress event from an expert council consultation.
    Council(CouncilEvent),
}

/// Progress events emitted during a multi-expert council consultation.
///
/// These are broadcast via WebSocket so frontends can show real-time progress.
/// MCP clients won't see these (MCP can't stream), but WebSocket clients will.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum CouncilEvent {
    /// Coordinator created a research plan.
    PlanCreated {
        task_count: usize,
        roles: Vec<String>,
    },
    /// An expert started working on their assigned task.
    ExpertStarted { role: String, task: String },
    /// An expert recorded a finding via store_finding.
    FindingAdded { role: String, topic: String },
    /// An expert completed their task.
    ExpertComplete {
        role: String,
        finding_count: usize,
    },
    /// Coordinator completed review of all findings.
    ReviewComplete {
        consensus_count: usize,
        conflict_count: usize,
    },
    /// A delta round started to resolve conflicts.
    DeltaRoundStarted {
        round: usize,
        question_count: usize,
    },
    /// Final synthesis phase started.
    SynthesisStarted,
    /// Council consultation complete.
    Complete {
        total_findings: usize,
        rounds: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // ProjectContext tests
    // ============================================================================

    #[test]
    fn test_project_context_serialize() {
        let ctx = ProjectContext {
            id: 1,
            path: "/home/user/project".to_string(),
            name: Some("my-project".to_string()),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("/home/user/project"));
        assert!(json.contains("my-project"));
    }

    #[test]
    fn test_project_context_deserialize() {
        let json = r#"{"id": 42, "path": "/test/path", "name": "test"}"#;
        let ctx: ProjectContext = serde_json::from_str(json).unwrap();
        assert_eq!(ctx.id, 42);
        assert_eq!(ctx.path, "/test/path");
        assert_eq!(ctx.name, Some("test".to_string()));
    }

    #[test]
    fn test_project_context_name_optional() {
        let json = r#"{"id": 1, "path": "/test"}"#;
        let ctx: ProjectContext = serde_json::from_str(json).unwrap();
        assert_eq!(ctx.name, None);
    }

    // ============================================================================
    // MemoryFact tests
    // ============================================================================

    #[test]
    fn test_memory_fact_defaults() {
        let json = r#"{
            "id": 1,
            "project_id": null,
            "key": null,
            "content": "Test memory",
            "fact_type": "general",
            "category": null,
            "confidence": 0.9,
            "created_at": "2024-01-01T00:00:00Z"
        }"#;
        let fact: MemoryFact = serde_json::from_str(json).unwrap();
        assert_eq!(fact.session_count, 1); // default
        assert_eq!(fact.status, "candidate"); // default
        assert_eq!(fact.scope, "project"); // default
    }

    #[test]
    fn test_memory_fact_full() {
        let json = r#"{
            "id": 42,
            "project_id": 1,
            "key": "test_key",
            "content": "Important fact",
            "fact_type": "preference",
            "category": "coding",
            "confidence": 0.95,
            "created_at": "2024-01-01T00:00:00Z",
            "session_count": 5,
            "first_session_id": "session-1",
            "last_session_id": "session-5",
            "status": "confirmed",
            "user_id": "user-123",
            "scope": "team",
            "team_id": 10
        }"#;
        let fact: MemoryFact = serde_json::from_str(json).unwrap();
        assert_eq!(fact.id, 42);
        assert_eq!(fact.session_count, 5);
        assert_eq!(fact.status, "confirmed");
        assert_eq!(fact.scope, "team");
        assert_eq!(fact.team_id, Some(10));
    }

    // ============================================================================
    // WsEvent tests
    // ============================================================================

    #[test]
    fn test_ws_event_tool_start_serialize() {
        let event = WsEvent::ToolStart {
            tool_name: "search_code".to_string(),
            arguments: serde_json::json!({"query": "test"}),
            call_id: "call-123".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"tool_start\""));
        assert!(json.contains("search_code"));
        assert!(json.contains("call-123"));
    }

    #[test]
    fn test_ws_event_tool_result_serialize() {
        let event = WsEvent::ToolResult {
            tool_name: "search_code".to_string(),
            result: "Found 5 matches".to_string(),
            success: true,
            call_id: "call-123".to_string(),
            duration_ms: 150,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"duration_ms\":150"));
    }

    #[test]
    fn test_ws_event_agent_response_serialize() {
        let event = WsEvent::AgentResponse {
            in_reply_to: "msg-456".to_string(),
            from: AgentRole::Mira,
            content: "Here is my response".to_string(),
            complete: true,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"agent_response\""));
        assert!(json.contains("\"from\":\"mira\""));
        assert!(json.contains("\"complete\":true"));
    }

    #[test]
    fn test_ws_event_deserialize_tool_start() {
        let json = r#"{
            "type": "tool_start",
            "tool_name": "index",
            "arguments": {"path": "/test"},
            "call_id": "abc"
        }"#;
        let event: WsEvent = serde_json::from_str(json).unwrap();
        match event {
            WsEvent::ToolStart {
                tool_name, call_id, ..
            } => {
                assert_eq!(tool_name, "index");
                assert_eq!(call_id, "abc");
            }
            _ => panic!("Expected ToolStart"),
        }
    }
}
