// src/llm/provider/mod.rs
// LLM Provider trait and type definitions for multi-provider support
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod claude;
pub mod openai;
pub mod deepseek;
pub mod conversion;

/// Message format for all providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Unified response from any provider
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub content: String,
    pub thinking: Option<String>,
    pub metadata: ProviderMetadata,
}

/// Metadata returned by provider
#[derive(Debug, Clone)]
pub struct ProviderMetadata {
    pub model_version: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub thinking_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub latency_ms: i64,
    pub finish_reason: Option<String>,
}

/// Universal LLM provider interface
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider name for logging/debugging
    fn name(&self) -> &'static str;
    
    /// Chat completion with optional thinking/reasoning
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        system: String,
        thinking_budget: Option<u32>,
    ) -> Result<ProviderResponse>;
    
    /// Chat with tool support (Claude & GPT-5 only)
    /// Returns raw response in Claude-compatible format
    async fn chat_with_tools(
        &self,
        _messages: Vec<ChatMessage>,
        _system: String,
        _tools: Vec<Value>,
        _tool_choice: Option<Value>,  // NEW: Optional forced tool selection
    ) -> Result<Value> {
        // Default: not supported
        Err(anyhow::anyhow!("{} does not support tool calling", self.name()))
    }
}
