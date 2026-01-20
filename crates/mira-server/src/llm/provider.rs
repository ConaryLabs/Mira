// crates/mira-server/src/llm/provider.rs
// LLM provider abstraction layer

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

use super::{ChatResult, Message, Tool};

/// LLM provider types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    DeepSeek,
    OpenAi,
    Gemini,
    Ollama, // Reserved for local sovereignty - not implemented yet
}

impl Provider {
    /// Parse provider from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "deepseek" => Some(Self::DeepSeek),
            "openai" => Some(Self::OpenAi),
            "gemini" => Some(Self::Gemini),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }

    /// Get the environment variable name for this provider's API key
    pub fn api_key_env_var(&self) -> &'static str {
        match self {
            Self::DeepSeek => "DEEPSEEK_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::Ollama => "OLLAMA_HOST", // Ollama uses host, not API key
        }
    }

    /// Default model for this provider
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek-reasoner",
            Self::OpenAi => "gpt-5.2",
            Self::Gemini => "gemini-3-pro-preview",
            Self::Ollama => "llama3.3",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeepSeek => write!(f, "deepseek"),
            Self::OpenAi => write!(f, "openai"),
            Self::Gemini => write!(f, "gemini"),
            Self::Ollama => write!(f, "ollama"),
        }
    }
}

/// Normalized usage statistics across all providers
#[derive(Debug, Clone, Default)]
pub struct NormalizedUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl NormalizedUsage {
    pub fn new(prompt: u32, completion: u32) -> Self {
        Self {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
        }
    }
}

/// Trait for LLM clients - all providers must implement this
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a chat completion request
    async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatResult>;

    /// Send a stateful chat request with optional previous response ID for continuation.
    /// This is used by providers like OpenAI that support stateful conversations via
    /// the Responses API. The previous_response_id allows the provider to maintain
    /// context including reasoning items across turns.
    ///
    /// Default implementation ignores previous_response_id and calls chat().
    async fn chat_stateful(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
        _previous_response_id: Option<&str>,
    ) -> Result<ChatResult> {
        self.chat(messages, tools).await
    }

    /// Whether this provider supports stateful conversations via previous_response_id.
    /// When true, the caller can send only new messages (tool results) on subsequent
    /// turns because the provider stores the full conversation context.
    /// When false, the caller must send the full message history every time.
    fn supports_stateful(&self) -> bool {
        false
    }

    /// Get the provider type
    fn provider_type(&self) -> Provider;

    /// Get the model name
    fn model_name(&self) -> String;

    /// Get normalized usage from the last request (if available)
    fn normalize_usage(&self, result: &ChatResult) -> NormalizedUsage {
        result
            .usage
            .as_ref()
            .map(|u| NormalizedUsage::new(u.prompt_tokens, u.completion_tokens))
            .unwrap_or_default()
    }
}
