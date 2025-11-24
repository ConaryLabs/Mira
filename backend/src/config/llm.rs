// src/config/llm.rs
// LLM provider configuration (GPT 5.1, DeepSeek, OpenAI)

use serde::{Deserialize, Serialize};
use crate::llm::provider::ReasoningEffort;

/// DeepSeek dual-model orchestration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekConfig {
    pub enabled: bool,
    pub api_key: String,
    pub chat_model: String,
    pub reasoner_model: String,
    pub chat_max_tokens: usize,
    pub reasoner_max_tokens: usize,
    pub enable_orchestration: bool,
    pub complexity_threshold: f32,
}

impl DeepSeekConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("USE_DEEPSEEK_CODEGEN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            api_key: super::helpers::env_or("DEEPSEEK_API_KEY", ""),
            chat_model: super::helpers::env_or("DEEPSEEK_CHAT_MODEL", "deepseek-chat"),
            reasoner_model: super::helpers::env_or("DEEPSEEK_REASONER_MODEL", "deepseek-reasoner"),
            chat_max_tokens: super::helpers::env_usize("DEEPSEEK_CHAT_MAX_TOKENS", 8192),
            reasoner_max_tokens: super::helpers::env_usize("DEEPSEEK_REASONER_MAX_TOKENS", 32768),
            enable_orchestration: std::env::var("DEEPSEEK_ENABLE_ORCHESTRATION")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            complexity_threshold: std::env::var("DEEPSEEK_COMPLEXITY_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.7),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.enabled && self.api_key.is_empty() {
            return Err(anyhow::anyhow!(
                "DEEPSEEK_API_KEY is required when DeepSeek is enabled"
            ));
        }

        if self.chat_max_tokens > 8192 {
            return Err(anyhow::anyhow!(
                "DEEPSEEK_CHAT_MAX_TOKENS cannot exceed 8192 (model limit)"
            ));
        }

        if self.reasoner_max_tokens > 65536 {
            return Err(anyhow::anyhow!(
                "DEEPSEEK_REASONER_MAX_TOKENS cannot exceed 65536 (model limit)"
            ));
        }

        if self.complexity_threshold < 0.0 || self.complexity_threshold > 1.0 {
            return Err(anyhow::anyhow!(
                "DEEPSEEK_COMPLEXITY_THRESHOLD must be between 0.0 and 1.0"
            ));
        }

        Ok(())
    }
}

/// GPT 5.1 configuration with reasoning effort
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpt5Config {
    pub enabled: bool,
    pub api_key: String,
    pub model: String,
    pub default_reasoning_effort: ReasoningEffort,
}

impl Gpt5Config {
    pub fn from_env() -> Self {
        let reasoning_str = super::helpers::env_or("GPT5_REASONING_DEFAULT", "medium");
        let default_reasoning_effort = match reasoning_str.to_lowercase().as_str() {
            "low" | "minimum" => ReasoningEffort::Minimum,
            "high" => ReasoningEffort::High,
            _ => ReasoningEffort::Medium,
        };

        Self {
            enabled: std::env::var("USE_GPT5")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            api_key: super::helpers::env_or("OPENAI_API_KEY", ""),
            model: super::helpers::env_or("GPT5_MODEL", "gpt-5.1"),
            default_reasoning_effort,
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.enabled && self.api_key.is_empty() {
            return Err(anyhow::anyhow!(
                "OPENAI_API_KEY is required when GPT 5.1 is enabled"
            ));
        }

        Ok(())
    }
}

/// OpenAI configuration (for embeddings and images only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiConfig {
    pub api_key: String,
    pub embedding_model: String,
    pub timeout: u64,
}

impl OpenAiConfig {
    pub fn from_env() -> Self {
        Self {
            api_key: super::helpers::require_env("OPENAI_API_KEY"),
            embedding_model: super::helpers::env_or(
                "OPENAI_EMBEDDING_MODEL",
                "text-embedding-3-large",
            ),
            timeout: super::helpers::require_env_parsed("OPENAI_TIMEOUT"),
        }
    }
}
