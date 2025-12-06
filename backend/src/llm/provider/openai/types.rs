// src/llm/provider/openai/types.rs
// Type definitions for OpenAI GPT-5.1 provider

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// OpenAI model variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAIModel {
    /// GPT-5.1 - Main model for voice/chat tier
    Gpt51,
    /// GPT-5.1 Mini - Fast tier for simple tasks
    Gpt51Mini,
    /// GPT-5.1-Codex-Max - Code tier for code-heavy tasks and agentic tier for long-running
    Gpt51CodexMax,
}

/// Reasoning effort level for models that support extended thinking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    /// Quick reasoning for simple tasks
    Medium,
    /// Standard reasoning for most tasks
    High,
    /// Extended thinking for complex, long-running tasks (Codex-Max only)
    #[serde(rename = "xhigh")]
    XHigh,
}

impl Default for ReasoningEffort {
    fn default() -> Self {
        ReasoningEffort::High
    }
}

impl std::fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReasoningEffort::Medium => write!(f, "medium"),
            ReasoningEffort::High => write!(f, "high"),
            ReasoningEffort::XHigh => write!(f, "xhigh"),
        }
    }
}

/// Reasoning configuration for API requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    pub effort: ReasoningEffort,
}

impl OpenAIModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            OpenAIModel::Gpt51 => "gpt-5.1",
            OpenAIModel::Gpt51Mini => "gpt-5.1-mini",
            OpenAIModel::Gpt51CodexMax => "gpt-5.1-codex-max",
        }
    }

    /// Get the display name for this model
    pub fn display_name(&self) -> &'static str {
        match self {
            OpenAIModel::Gpt51 => "GPT-5.1",
            OpenAIModel::Gpt51Mini => "GPT-5.1 Mini",
            OpenAIModel::Gpt51CodexMax => "GPT-5.1-Codex-Max",
        }
    }

    /// Get max context window size
    /// Note: Codex-Max uses compaction for effectively unlimited context,
    /// but we report 1M as a conservative working limit
    pub fn max_context_tokens(&self) -> i64 {
        match self {
            OpenAIModel::Gpt51 => 272_000,
            OpenAIModel::Gpt51Mini => 400_000,
            OpenAIModel::Gpt51CodexMax => 1_000_000, // Compaction handles overflow
        }
    }

    /// Get max output tokens
    pub fn max_output_tokens(&self) -> i64 {
        match self {
            OpenAIModel::Gpt51 => 128_000,
            OpenAIModel::Gpt51Mini => 128_000,
            OpenAIModel::Gpt51CodexMax => 128_000,
        }
    }

    /// Check if this model supports reasoning effort configuration
    pub fn supports_reasoning(&self) -> bool {
        matches!(self, OpenAIModel::Gpt51CodexMax)
    }
}

impl Default for OpenAIModel {
    fn default() -> Self {
        OpenAIModel::Gpt51
    }
}

impl std::fmt::Display for OpenAIModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// OpenAI chat completion request
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Reasoning effort configuration (Codex-Max models only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
}

/// Chat message for OpenAI API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Tool call in assistant message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCallMessage,
}

/// Function call details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallMessage {
    pub name: String,
    pub arguments: String,
}

/// Tool definition for OpenAI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// Function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// OpenAI chat completion response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

/// Choice in completion response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: i64,
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

/// Message in response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallMessage>>,
}

/// Token usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

/// Streaming chunk
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

/// Choice in streaming chunk
#[derive(Debug, Clone, Deserialize)]
pub struct ChunkChoice {
    pub index: i64,
    pub delta: DeltaMessage,
    pub finish_reason: Option<String>,
}

/// Delta message in streaming
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaMessage {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<DeltaToolCall>>,
}

/// Tool call delta in streaming
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaToolCall {
    pub index: i64,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type", default)]
    pub call_type: Option<String>,
    #[serde(default)]
    pub function: Option<DeltaFunction>,
}

/// Function delta in streaming
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaFunction {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

/// OpenAI API error response
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

/// Error detail
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorDetail {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub code: Option<String>,
}
