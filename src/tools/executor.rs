// src/tools/executor.rs
// Minimal tool utilities for Claude tool calling
// The actual tool schemas and execution are in src/llm/structured/

use serde::Serialize;

/// Tool event types for streaming responses
#[derive(Debug, Clone, Serialize)]
pub enum ToolEvent {
    ContentChunk(String),
    ToolExecution { tool_name: String, status: String },
    ToolResult { tool_name: String, result: serde_json::Value },
    Complete { metadata: Option<serde_json::Value> },
    Error(String),
}

/// Simple tool executor placeholder
/// Actual tool calling happens via Claude's API in src/llm/structured/
pub struct ToolExecutor;

impl ToolExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}
