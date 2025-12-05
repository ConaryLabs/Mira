// src/config/llm.rs
// LLM provider configuration - Gemini 3 Pro

use serde::{Deserialize, Serialize};
pub use crate::llm::provider::ThinkingLevel;

/// Gemini 3 Pro API limits (Tier 1 - Paid)
/// Reference: https://ai.google.dev/gemini-api/docs/models/gemini-v2
#[derive(Debug, Clone, Copy)]
pub struct GeminiLimits {
    /// Maximum input context window (1M tokens)
    pub context_window: usize,
    /// Maximum output tokens per request (64K tokens)
    pub max_output_tokens: usize,
    /// Context size threshold for higher pricing tier (200K tokens)
    pub large_context_threshold: usize,
    /// Requests per minute (Tier 1)
    pub rpm_limit: u32,
    /// Tokens per minute (Tier 1)
    pub tpm_limit: usize,
    /// Requests per day (Tier 1)
    pub rpd_limit: usize,
}

impl Default for GeminiLimits {
    fn default() -> Self {
        Self {
            context_window: 1_000_000,
            max_output_tokens: 65_536,
            large_context_threshold: 200_000,
            rpm_limit: 50,
            tpm_limit: 1_000_000,
            rpd_limit: 1_000,
        }
    }
}

impl GeminiLimits {
    /// Get the default Gemini 3 Pro limits
    pub fn gemini_3_pro() -> Self {
        Self::default()
    }
}

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
