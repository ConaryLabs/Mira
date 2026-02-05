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
    Gemini,
    Ollama,   // Reserved for local sovereignty - not implemented yet
    Sampling, // MCP sampling — forwards to host client (Claude Code)
}

impl Provider {
    /// Parse provider from string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "deepseek" => Some(Self::DeepSeek),
            "gemini" => Some(Self::Gemini),
            "ollama" => Some(Self::Ollama),
            "sampling" => Some(Self::Sampling),
            _ => None,
        }
    }

    /// Get the environment variable name for this provider's API key
    pub fn api_key_env_var(&self) -> &'static str {
        match self {
            Self::DeepSeek => "DEEPSEEK_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::Ollama => "OLLAMA_HOST", // Ollama uses host, not API key
            Self::Sampling => "N/A",       // No API key — uses MCP sampling
        }
    }

    /// Default model for this provider
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek-reasoner",
            Self::Gemini => "gemini-3-pro-preview",
            Self::Ollama => "llama3.3",
            Self::Sampling => "mcp-sampling",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeepSeek => write!(f, "deepseek"),
            Self::Gemini => write!(f, "gemini"),
            Self::Ollama => write!(f, "ollama"),
            Self::Sampling => write!(f, "sampling"),
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
    async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult>;

    /// Send a stateful chat request with optional previous response ID for continuation.
    /// This is used by providers that support stateful conversations via
    /// a continuation API. The previous_response_id allows the provider to maintain
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

    /// Token budget for this provider's context window (0 = no budget management).
    /// Used by the agentic loop to truncate messages before sending.
    fn context_budget(&self) -> u64 {
        0
    }

    /// Whether this provider supports automatic context budget management.
    fn supports_context_budget(&self) -> bool {
        self.context_budget() > 0
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

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // Provider::from_str tests
    // ============================================================================

    #[test]
    fn test_provider_from_str_deepseek() {
        assert_eq!(Provider::from_str("deepseek"), Some(Provider::DeepSeek));
        assert_eq!(Provider::from_str("DeepSeek"), Some(Provider::DeepSeek));
        assert_eq!(Provider::from_str("DEEPSEEK"), Some(Provider::DeepSeek));
    }

    #[test]
    fn test_provider_from_str_gemini() {
        assert_eq!(Provider::from_str("gemini"), Some(Provider::Gemini));
        assert_eq!(Provider::from_str("Gemini"), Some(Provider::Gemini));
        assert_eq!(Provider::from_str("GEMINI"), Some(Provider::Gemini));
    }

    #[test]
    fn test_provider_from_str_ollama() {
        assert_eq!(Provider::from_str("ollama"), Some(Provider::Ollama));
        assert_eq!(Provider::from_str("Ollama"), Some(Provider::Ollama));
    }

    #[test]
    fn test_provider_from_str_invalid() {
        assert_eq!(Provider::from_str("invalid"), None);
        assert_eq!(Provider::from_str("gpt"), None);
        assert_eq!(Provider::from_str("claude"), None);
        assert_eq!(Provider::from_str(""), None);
    }

    // ============================================================================
    // Provider::api_key_env_var tests
    // ============================================================================

    #[test]
    fn test_provider_api_key_env_var() {
        assert_eq!(Provider::DeepSeek.api_key_env_var(), "DEEPSEEK_API_KEY");
        assert_eq!(Provider::Gemini.api_key_env_var(), "GEMINI_API_KEY");
        assert_eq!(Provider::Ollama.api_key_env_var(), "OLLAMA_HOST");
    }

    // ============================================================================
    // Provider::default_model tests
    // ============================================================================

    #[test]
    fn test_provider_default_model() {
        assert_eq!(Provider::DeepSeek.default_model(), "deepseek-reasoner");
        assert_eq!(Provider::Gemini.default_model(), "gemini-3-pro-preview");
        assert_eq!(Provider::Ollama.default_model(), "llama3.3");
    }

    // ============================================================================
    // Provider Display tests
    // ============================================================================

    #[test]
    fn test_provider_display() {
        assert_eq!(format!("{}", Provider::DeepSeek), "deepseek");
        assert_eq!(format!("{}", Provider::Gemini), "gemini");
        assert_eq!(format!("{}", Provider::Ollama), "ollama");
    }

    // ============================================================================
    // NormalizedUsage tests
    // ============================================================================

    #[test]
    fn test_normalized_usage_new() {
        let usage = NormalizedUsage::new(100, 50);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }
}
