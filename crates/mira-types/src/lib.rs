// crates/mira-types/src/lib.rs
// Shared types for Mira (native + WASM compatible)
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
    // Evidence-based memory fields
    #[serde(default = "default_session_count")]
    pub session_count: i32,
    #[serde(default)]
    pub first_session_id: Option<String>,
    #[serde(default)]
    pub last_session_id: Option<String>,
    #[serde(default = "default_status")]
    pub status: String,
    // Multi-user memory sharing fields
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default = "default_scope")]
    pub scope: String,
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

/// WebSocket event for MCP tool broadcasting
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
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

    // Agent collaboration
    AgentResponse {
        in_reply_to: String,
        from: AgentRole,
        content: String,
        complete: bool,
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

    #[test]
    fn test_memory_fact_clone() {
        let fact = MemoryFact {
            id: 1,
            project_id: Some(1),
            key: Some("key".to_string()),
            content: "content".to_string(),
            fact_type: "general".to_string(),
            category: Some("cat".to_string()),
            confidence: 0.9,
            created_at: "2024-01-01".to_string(),
            session_count: 1,
            first_session_id: None,
            last_session_id: None,
            status: "candidate".to_string(),
            user_id: None,
            scope: "project".to_string(),
            team_id: None,
        };
        let cloned = fact.clone();
        assert_eq!(fact.id, cloned.id);
        assert_eq!(fact.content, cloned.content);
    }

    // ============================================================================
    // AgentRole tests
    // ============================================================================

    #[test]
    fn test_agent_role_serialize() {
        assert_eq!(serde_json::to_string(&AgentRole::Mira).unwrap(), "\"mira\"");
        assert_eq!(serde_json::to_string(&AgentRole::Claude).unwrap(), "\"claude\"");
    }

    #[test]
    fn test_agent_role_deserialize() {
        let mira: AgentRole = serde_json::from_str("\"mira\"").unwrap();
        assert_eq!(mira, AgentRole::Mira);

        let claude: AgentRole = serde_json::from_str("\"claude\"").unwrap();
        assert_eq!(claude, AgentRole::Claude);
    }

    #[test]
    fn test_agent_role_equality() {
        assert_eq!(AgentRole::Mira, AgentRole::Mira);
        assert_eq!(AgentRole::Claude, AgentRole::Claude);
        assert_ne!(AgentRole::Mira, AgentRole::Claude);
    }

    #[test]
    fn test_agent_role_copy() {
        let role = AgentRole::Mira;
        let copied = role;
        assert_eq!(role, copied);
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
            WsEvent::ToolStart { tool_name, call_id, .. } => {
                assert_eq!(tool_name, "index");
                assert_eq!(call_id, "abc");
            }
            _ => panic!("Expected ToolStart"),
        }
    }

    #[test]
    fn test_ws_event_equality() {
        let event1 = WsEvent::ToolResult {
            tool_name: "test".to_string(),
            result: "ok".to_string(),
            success: true,
            call_id: "1".to_string(),
            duration_ms: 100,
        };
        let event2 = WsEvent::ToolResult {
            tool_name: "test".to_string(),
            result: "ok".to_string(),
            success: true,
            call_id: "1".to_string(),
            duration_ms: 100,
        };
        assert_eq!(event1, event2);
    }

    // ============================================================================
    // Default function tests
    // ============================================================================

    #[test]
    fn test_default_session_count() {
        assert_eq!(default_session_count(), 1);
    }

    #[test]
    fn test_default_status() {
        assert_eq!(default_status(), "candidate");
    }

    #[test]
    fn test_default_scope() {
        assert_eq!(default_scope(), "project");
    }
}
