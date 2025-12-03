// src/llm/provider/gemini3/types.rs
// Type definitions for Gemini 3 provider

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Thinking level for Gemini 3 (only Low and High available)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingLevel {
    Low,
    High,
}

impl ThinkingLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThinkingLevel::Low => "low",
            ThinkingLevel::High => "high",
        }
    }
}

impl Default for ThinkingLevel {
    fn default() -> Self {
        ThinkingLevel::High
    }
}

/// Response from tool calling API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
    pub tokens_input: i64,
    pub tokens_output: i64,
    /// Thought signature for multi-turn conversations (MUST be passed back)
    pub thought_signature: Option<String>,
}

/// Individual tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Request to generate code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenRequest {
    pub path: String,
    pub description: String,
    pub language: String,
    pub framework: Option<String>,
    pub dependencies: Vec<String>,
    pub style_guide: Option<String>,
    pub context: String,
}

/// Response from code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenResponse {
    pub artifact: CodeArtifact,
    pub tokens_input: i64,
    pub tokens_output: i64,
}

/// Code artifact generated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeArtifact {
    pub path: String,
    pub content: String,
    pub language: String,
    #[serde(default)]
    pub explanation: Option<String>,
}
