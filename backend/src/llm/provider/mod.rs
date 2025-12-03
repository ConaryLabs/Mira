// src/llm/provider/mod.rs
// LLM Provider trait - Gemini 3 Pro primary provider

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;

// Gemini 3 provider (modularized)
pub mod gemini3;
pub mod gemini_embeddings;
pub mod stream;

// Export Gemini providers (primary)
pub use gemini3::{
    build_user_prompt, CodeArtifact, CodeGenRequest, CodeGenResponse, Gemini3Pricing,
    Gemini3Provider, ThinkingLevel, ToolCall, ToolCallResponse,
};
pub use gemini_embeddings::GeminiEmbeddings;
pub use stream::StreamEvent;

/// Tool call information for assistant messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Simple message format for all providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,

    /// For tool response messages - links response to specific tool call
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// For assistant messages that request tool calls
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallInfo>>,

    /// Thought signature for Gemini 3 multi-turn conversations
    /// CRITICAL: Must be captured and passed back in subsequent turns
    /// for function calling to work correctly
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

impl Message {
    pub fn user(content: String) -> Self {
        Self {
            role: "user".to_string(),
            content,
            tool_call_id: None,
            tool_calls: None,
            thought_signature: None,
        }
    }

    pub fn assistant(content: String) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_call_id: None,
            tool_calls: None,
            thought_signature: None,
        }
    }

    pub fn system(content: String) -> Self {
        Self {
            role: "system".to_string(),
            content,
            tool_call_id: None,
            tool_calls: None,
            thought_signature: None,
        }
    }

    pub fn tool_result(call_id: String, output: String) -> Self {
        Self {
            role: "tool".to_string(),
            content: output,
            tool_call_id: Some(call_id),
            tool_calls: None,
            thought_signature: None,
        }
    }

    pub fn assistant_with_tool_calls(content: String, tool_calls: Vec<ToolCallInfo>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_call_id: None,
            tool_calls: Some(tool_calls),
            thought_signature: None,
        }
    }

    /// Create an assistant message with tool calls and thought signature
    /// Used for Gemini 3 multi-turn function calling
    pub fn assistant_with_tool_calls_and_signature(
        content: String,
        tool_calls: Vec<ToolCallInfo>,
        thought_signature: Option<String>,
    ) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_call_id: None,
            tool_calls: Some(tool_calls),
            thought_signature,
        }
    }

    /// Set the thought signature on an existing message
    pub fn with_thought_signature(mut self, signature: Option<String>) -> Self {
        self.thought_signature = signature;
        self
    }
}

/// Token usage tracking across all providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: i64,
    pub output: i64,
    pub reasoning: i64, // For thinking tokens (Gemini 3)
    pub cached: i64,    // For cached tokens
}

/// Basic chat response (no tools)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub content: String,
    pub model: String,
    pub tokens: TokenUsage,
    pub latency_ms: i64,
}

/// Function call from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Tool calling response with function calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResponse {
    pub id: String,                        // Response ID (for multi-turn)
    pub text_output: String,               // Text response
    pub function_calls: Vec<FunctionCall>, // Function calls made
    pub tokens: TokenUsage,
    pub latency_ms: i64,
    pub raw_response: Value, // Full API response
}

/// Context for multi-turn conversations
#[derive(Debug, Clone)]
pub enum ToolContext {
    // Reserved for future multi-turn context if needed
}

/// Universal LLM provider interface
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider name for logging
    fn name(&self) -> &'static str;

    /// Downcast to concrete type (for accessing provider-specific methods)
    fn as_any(&self) -> &dyn Any;

    /// Basic chat (no tools)
    async fn chat(&self, messages: Vec<Message>, system: String) -> Result<Response>;

    /// Chat with tool calling
    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        context: Option<ToolContext>,
    ) -> Result<ToolResponse>;

    async fn stream(
        &self,
        _messages: Vec<Message>,
        _system: String,
    ) -> Result<Box<dyn futures::Stream<Item = Result<String>> + Send + Unpin>> {
        Err(anyhow::anyhow!(
            "{} does not support streaming",
            self.name()
        ))
    }
}
