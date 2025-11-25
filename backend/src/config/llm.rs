// src/config/llm.rs
// LLM provider configuration (GPT 5.1, OpenAI)

use serde::{Deserialize, Serialize};
pub use crate::llm::provider::ReasoningEffort;

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
