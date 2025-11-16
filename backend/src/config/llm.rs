// src/config/llm.rs
// LLM provider configuration (GPT-5, DeepSeek, OpenAI)

use serde::{Deserialize, Serialize};

/// GPT-5 Responses API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpt5Config {
    pub api_key: String,
    pub model: String,
    pub max_tokens: usize,
    pub verbosity: String,
    pub reasoning: String,
}

impl Gpt5Config {
    pub fn from_env() -> Self {
        Self {
            api_key: super::helpers::require_env("GPT5_API_KEY"),
            model: super::helpers::env_or("GPT5_MODEL", "gpt-5"),
            max_tokens: super::helpers::env_usize("GPT5_MAX_TOKENS", 128000),
            verbosity: super::helpers::env_or("GPT5_VERBOSITY", "medium"),
            reasoning: super::helpers::env_or("GPT5_REASONING", "medium"),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if !["low", "medium", "high"].contains(&self.verbosity.as_str()) {
            return Err(anyhow::anyhow!(
                "Invalid GPT5_VERBOSITY: must be low/medium/high"
            ));
        }

        if !["minimal", "low", "medium", "high"].contains(&self.reasoning.as_str()) {
            return Err(anyhow::anyhow!(
                "Invalid GPT5_REASONING: must be minimal/low/medium/high"
            ));
        }

        Ok(())
    }
}

/// DeepSeek code generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekConfig {
    pub enabled: bool,
    pub api_key: String,
}

impl DeepSeekConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("USE_DEEPSEEK_CODEGEN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            api_key: super::helpers::env_or("DEEPSEEK_API_KEY", ""),
        }
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
