// src/config/llm.rs
// Minimal LLM config for embedding API access only (no LLM orchestration)

use serde::{Deserialize, Serialize};

/// Thinking level - kept for backward compatibility with any remaining references
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ThinkingLevel {
    Low,
    #[default]
    High,
}

/// Context budget configuration (minimal, may be removed later)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudgetConfig {
    pub enforce_standard_tier: bool,
    pub max_context_tokens: usize,
    pub enable_threshold_warnings: bool,
    pub warning_threshold_percent: u8,
}

impl Default for ContextBudgetConfig {
    fn default() -> Self {
        Self {
            enforce_standard_tier: false,
            max_context_tokens: 0,
            enable_threshold_warnings: true,
            warning_threshold_percent: 90,
        }
    }
}

impl ContextBudgetConfig {
    pub fn from_env() -> Self {
        Self::default()
    }
}

/// OpenAI configuration - only used for embeddings API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIConfig {
    pub enabled: bool,
    pub api_key: String,
    pub embedding_model: String,
    pub embedding_dimensions: usize,
    pub timeout_seconds: u64,
    // Keep these for backward compatibility but they're unused
    pub fast_model: String,
    pub voice_model: String,
    pub code_model: String,
    pub agentic_model: String,
}

impl OpenAIConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("OPENAI_API_KEY").is_ok(),
            api_key: super::helpers::env_or("OPENAI_API_KEY", ""),
            embedding_model: super::helpers::env_or("MIRA_EMBED_MODEL", "text-embedding-3-large"),
            embedding_dimensions: super::helpers::env_or("MIRA_EMBED_DIMENSIONS", "3072")
                .parse()
                .unwrap_or(3072),
            timeout_seconds: super::helpers::env_or("OPENAI_TIMEOUT", "60")
                .parse()
                .unwrap_or(60),
            fast_model: String::new(),
            voice_model: String::new(),
            code_model: String::new(),
            agentic_model: String::new(),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.enabled && self.api_key.is_empty() {
            return Err(anyhow::anyhow!(
                "OPENAI_API_KEY is required for embeddings"
            ));
        }
        Ok(())
    }
}

/// Gemini config - kept for backward compatibility but unused in power suit mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    pub enabled: bool,
    pub api_key: String,
    pub model: String,
    pub embedding_model: String,
    pub default_thinking_level: ThinkingLevel,
}

impl GeminiConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            model: String::new(),
            embedding_model: String::new(),
            default_thinking_level: ThinkingLevel::default(),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
