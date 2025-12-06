// src/config/llm.rs
// LLM provider configuration - OpenAI GPT-5.1

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

/// Context budget configuration
/// Controls whether to enforce staying within standard pricing tier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudgetConfig {
    /// Enforce staying in standard pricing tier (<200k context)
    /// If true, context will be truncated to avoid large context pricing
    pub enforce_standard_tier: bool,
    /// Maximum context tokens (0 = use model maximum)
    /// If set, context will be truncated to this limit regardless of tier
    pub max_context_tokens: usize,
    /// Enable proactive warnings when approaching pricing threshold
    pub enable_threshold_warnings: bool,
    /// Custom warning threshold (90% of 200k = 180k by default)
    pub warning_threshold_percent: u8,
}

impl Default for ContextBudgetConfig {
    fn default() -> Self {
        Self {
            enforce_standard_tier: false,
            max_context_tokens: 0, // No limit by default
            enable_threshold_warnings: true,
            warning_threshold_percent: 90,
        }
    }
}

impl ContextBudgetConfig {
    pub fn from_env() -> Self {
        Self {
            enforce_standard_tier: super::helpers::env_or("ENFORCE_STANDARD_PRICING_TIER", "false")
                .parse()
                .unwrap_or(false),
            max_context_tokens: super::helpers::env_or("MAX_CONTEXT_TOKENS", "0")
                .parse()
                .unwrap_or(0),
            enable_threshold_warnings: super::helpers::env_or("ENABLE_CONTEXT_WARNINGS", "true")
                .parse()
                .unwrap_or(true),
            warning_threshold_percent: super::helpers::env_or("CONTEXT_WARNING_THRESHOLD_PERCENT", "90")
                .parse()
                .unwrap_or(90),
        }
    }

    /// Get the effective maximum context tokens
    pub fn effective_max_context(&self) -> usize {
        if self.max_context_tokens > 0 {
            self.max_context_tokens
        } else if self.enforce_standard_tier {
            200_000 // Stay under large context threshold
        } else {
            1_000_000 // Full model context window
        }
    }
}

/// OpenAI GPT-5.1 configuration for 4-tier model routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIConfig {
    pub enabled: bool,
    pub api_key: String,
    /// Model for Fast tier (default: gpt-5.1-codex-mini) - file ops, search
    pub fast_model: String,
    /// Model for Voice tier (default: gpt-5.1) - user chat, explanations
    pub voice_model: String,
    /// Model for Code tier (default: gpt-5.1-codex-max) - code generation, refactoring
    pub code_model: String,
    /// Model for Agentic tier (default: gpt-5.1-codex-max) - long-running tasks
    pub agentic_model: String,
    /// Embedding model (default: text-embedding-3-large, 3072 dimensions)
    pub embedding_model: String,
    /// Embedding dimensions
    pub embedding_dimensions: usize,
    /// Timeout for OpenAI API calls in seconds
    pub timeout_seconds: u64,
}

impl OpenAIConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("MODEL_ROUTER_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            api_key: super::helpers::env_or("OPENAI_API_KEY", ""),
            fast_model: super::helpers::env_or("MODEL_FAST", "gpt-5.1-codex-mini"),
            voice_model: super::helpers::env_or("MODEL_VOICE", "gpt-5.1"),
            code_model: super::helpers::env_or("MODEL_CODE", "gpt-5.1-codex-max"),
            agentic_model: super::helpers::env_or("MODEL_AGENTIC", "gpt-5.1-codex-max"),
            embedding_model: super::helpers::env_or("MIRA_EMBED_MODEL", "text-embedding-3-large"),
            embedding_dimensions: super::helpers::env_or("MIRA_EMBED_DIMENSIONS", "3072")
                .parse()
                .unwrap_or(3072),
            timeout_seconds: super::helpers::env_or("OPENAI_TIMEOUT", "600")
                .parse()
                .unwrap_or(600),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.enabled && self.api_key.is_empty() {
            return Err(anyhow::anyhow!(
                "OPENAI_API_KEY is required for OpenAI GPT-5.1"
            ));
        }
        Ok(())
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
