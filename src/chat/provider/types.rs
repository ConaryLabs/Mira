//! Unified types for provider abstraction
//!
//! These types work across all providers, with provider-specific
//! implementations handling the translation.

use serde::{Deserialize, Serialize};

/// A message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

/// Message roles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Unified chat request
#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// The model to use
    pub model: String,

    /// System prompt / instructions
    pub system: String,

    /// Message history (for client-state providers)
    pub messages: Vec<Message>,

    /// User's current input
    pub input: String,

    /// Previous response ID (for server-state providers like OpenAI)
    pub previous_response_id: Option<String>,

    /// Reasoning effort (low/medium/high) if supported
    pub reasoning_effort: Option<String>,

    /// Available tools
    pub tools: Vec<ToolDefinition>,

    /// Maximum output tokens (None = provider default)
    pub max_tokens: Option<u32>,
}

impl ChatRequest {
    /// Create a new chat request
    pub fn new(model: impl Into<String>, system: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            system: system.into(),
            messages: Vec::new(),
            input: input.into(),
            previous_response_id: None,
            reasoning_effort: None,
            tools: Vec::new(),
            max_tokens: None,
        }
    }

    /// Add message history
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    /// Set previous response ID (for OpenAI)
    pub fn with_previous_response_id(mut self, id: impl Into<String>) -> Self {
        self.previous_response_id = Some(id.into());
        self
    }

    /// Set reasoning effort
    pub fn with_reasoning(mut self, effort: impl Into<String>) -> Self {
        self.reasoning_effort = Some(effort.into());
        self
    }

    /// Add tools
    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    /// Set max tokens
    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }
}

/// Request to continue with tool results
#[derive(Debug, Clone)]
pub struct ToolContinueRequest {
    /// The model to use
    pub model: String,

    /// System prompt
    pub system: String,

    /// Previous response ID (for server-state)
    pub previous_response_id: Option<String>,

    /// Message history (for client-state)
    pub messages: Vec<Message>,

    /// Tool results
    pub tool_results: Vec<ToolResult>,

    /// Reasoning effort
    pub reasoning_effort: Option<String>,

    /// Available tools (for next iteration)
    pub tools: Vec<ToolDefinition>,
}

/// Result of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: String,
    pub name: String,
    pub output: String,
}

/// Streaming event from provider
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text delta
    TextDelta(String),

    /// Reasoning delta (for models with visible CoT)
    ReasoningDelta(String),

    /// Function call started
    FunctionCallStart {
        call_id: String,
        name: String,
    },

    /// Function call arguments delta
    FunctionCallDelta {
        call_id: String,
        arguments_delta: String,
    },

    /// Function call completed
    FunctionCallEnd {
        call_id: String,
    },

    /// Usage information
    Usage(Usage),

    /// Response ID (for server-state providers)
    ResponseId(String),

    /// Stream completed
    Done,

    /// Error occurred
    Error(String),
}

/// Unified chat response (non-streaming)
#[derive(Debug, Clone)]
pub struct ChatResponse {
    /// Response ID (for continuation)
    pub id: String,

    /// Generated text content
    pub text: String,

    /// Reasoning content (if exposed)
    pub reasoning: Option<String>,

    /// Tool calls requested
    pub tool_calls: Vec<ToolCall>,

    /// Usage information
    pub usage: Option<Usage>,

    /// Finish reason
    pub finish_reason: FinishReason,
}

/// A tool call from the model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

/// Token usage information
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub reasoning_tokens: u32,
    pub cached_tokens: u32,
}

impl Usage {
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.output_tokens + self.reasoning_tokens
    }

    pub fn cache_percentage(&self) -> Option<u32> {
        if self.input_tokens > 0 {
            Some((self.cached_tokens as f64 / self.input_tokens as f64 * 100.0) as u32)
        } else {
            None
        }
    }
}

/// Why the response finished
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural end of response
    Stop,
    /// Hit token limit
    Length,
    /// Tool call requested
    ToolCalls,
    /// Content filtered
    ContentFilter,
    /// Error occurred
    Error,
}

impl Default for FinishReason {
    fn default() -> Self {
        Self::Stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_request_builder() {
        let req = ChatRequest::new("gpt-5.2", "You are helpful", "Hello")
            .with_reasoning("high")
            .with_max_tokens(8000);

        assert_eq!(req.model, "gpt-5.2");
        assert_eq!(req.reasoning_effort, Some("high".into()));
        assert_eq!(req.max_tokens, Some(8000));
    }

    #[test]
    fn test_usage_calculations() {
        let usage = Usage {
            input_tokens: 1000,
            output_tokens: 500,
            reasoning_tokens: 100,
            cached_tokens: 800,
        };

        assert_eq!(usage.total_tokens(), 1600);
        assert_eq!(usage.cache_percentage(), Some(80));
    }
}
