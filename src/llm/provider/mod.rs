// src/llm/provider/mod.rs
// LLM Provider trait - clean, provider-agnostic interface
use async_trait::async_trait;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;

pub mod openai;
pub mod gpt5;
pub mod conversion;

// Export the embeddings client
pub use openai::OpenAiEmbeddings;

/// Simple message format for all providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Token usage tracking across all providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: i64,
    pub output: i64,
    pub reasoning: i64,  // For GPT-5
    pub cached: i64,     // For future use
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
    pub id: String,                         // Response ID (for multi-turn)
    pub text_output: String,                // Text response
    pub function_calls: Vec<FunctionCall>,  // Function calls made
    pub tokens: TokenUsage,
    pub latency_ms: i64,
    pub raw_response: Value,                // Full API response
}

/// Context for multi-turn conversations
#[derive(Debug, Clone)]
pub enum ToolContext {
    Gpt5 {
        previous_response_id: String,  // For GPT-5 multi-turn
    },
}

/// Universal LLM provider interface
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider name for logging
    fn name(&self) -> &'static str;
    
    /// Downcast to concrete type (for accessing provider-specific methods)
    fn as_any(&self) -> &dyn Any;
    
    /// Basic chat (no tools)
    async fn chat(
        &self,
        messages: Vec<Message>,
        system: String,
    ) -> Result<Response>;
    
    /// Chat with tool calling
    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        context: Option<ToolContext>,
    ) -> Result<ToolResponse>;
    
    /// Streaming chat (optional, default not implemented)
    async fn stream(
        &self,
        _messages: Vec<Message>,
        _system: String,
    ) -> Result<Box<dyn futures::Stream<Item = Result<String>> + Send + Unpin>> {
        Err(anyhow::anyhow!("{} does not support streaming", self.name()))
    }
}
