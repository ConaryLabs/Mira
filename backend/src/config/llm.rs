// src/config/llm.rs
// LLM provider configuration - Gemini 3 Pro

use serde::{Deserialize, Serialize};
pub use crate::llm::provider::ThinkingLevel;

/// Gemini 3 configuration with thinking level
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
        let thinking_str = super::helpers::env_or("GEMINI_THINKING_LEVEL", "high");
        let default_thinking_level = match thinking_str.to_lowercase().as_str() {
            "low" => ThinkingLevel::Low,
            _ => ThinkingLevel::High,
        };

        Self {
            enabled: std::env::var("USE_GEMINI")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            api_key: super::helpers::env_or("GOOGLE_API_KEY", ""),
            model: super::helpers::env_or("GEMINI_MODEL", "gemini-3-pro-preview"),
            embedding_model: super::helpers::env_or("GEMINI_EMBEDDING_MODEL", "gemini-embedding-001"),
            default_thinking_level,
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.enabled && self.api_key.is_empty() {
            return Err(anyhow::anyhow!(
                "GOOGLE_API_KEY is required when Gemini is enabled"
            ));
        }

        Ok(())
    }
}
