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
