// crates/mira-server/src/config/env.rs
// Environment-based configuration - single source of truth for all env vars

use tracing::{debug, info, warn};

/// API keys loaded from environment variables
#[derive(Debug, Clone, Default)]
pub struct ApiKeys {
    /// DeepSeek API key (DEEPSEEK_API_KEY)
    pub deepseek: Option<String>,
    /// Gemini/Google API key (GEMINI_API_KEY or GOOGLE_API_KEY)
    pub gemini: Option<String>,
    /// OpenAI API key (OPENAI_API_KEY) — used for embeddings
    pub openai: Option<String>,
    /// Brave Search API key (BRAVE_API_KEY)
    pub brave: Option<String>,
}

impl ApiKeys {
    /// Load API keys from environment variables (single source of truth)
    ///
    /// Set `MIRA_DISABLE_LLM=1` to suppress all LLM keys (forces heuristic fallbacks)
    pub fn from_env() -> Self {
        if parse_bool_env("MIRA_DISABLE_LLM").unwrap_or(false) {
            info!("MIRA_DISABLE_LLM is set — LLM providers disabled, using fallbacks");
            return Self {
                deepseek: None,
                gemini: None,
                openai: None,
                brave: Self::read_key("BRAVE_API_KEY"),
            };
        }

        let deepseek = Self::read_key("DEEPSEEK_API_KEY");
        let gemini = Self::read_key("GEMINI_API_KEY").or_else(|| Self::read_key("GOOGLE_API_KEY"));
        let openai = Self::read_key("OPENAI_API_KEY");
        let brave = Self::read_key("BRAVE_API_KEY");

        let keys = Self {
            deepseek,
            gemini,
            openai,
            brave,
        };
        keys.log_status();
        keys
    }

    /// Read a single API key from environment, filtering empty values
    fn read_key(name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|k| !k.trim().is_empty())
    }

    /// Check if web search is available (requires Brave key)
    pub fn has_web_search(&self) -> bool {
        self.brave.is_some()
    }

    /// Log which API keys are available (without exposing values)
    fn log_status(&self) {
        let mut available = Vec::new();
        if self.deepseek.is_some() {
            available.push("DeepSeek");
        }
        if self.gemini.is_some() {
            available.push("Gemini");
        }
        if self.openai.is_some() {
            available.push("OpenAI");
        }
        if self.brave.is_some() {
            available.push("Brave Search");
        }

        if available.is_empty() {
            warn!("No API keys configured - LLM features will be unavailable");
        } else {
            debug!(keys = ?available, "API keys loaded");
        }
    }

    /// Check if any LLM provider is available
    pub fn has_llm_provider(&self) -> bool {
        self.deepseek.is_some() || self.gemini.is_some()
    }

    /// Check if embeddings are available (requires OpenAI key)
    pub fn has_embeddings(&self) -> bool {
        self.openai.is_some()
    }

    /// Get a summary of available providers
    pub fn summary(&self) -> String {
        let mut providers = Vec::new();
        if self.deepseek.is_some() {
            providers.push("DeepSeek");
        }
        if self.gemini.is_some() {
            providers.push("Gemini");
        }
        if self.openai.is_some() {
            providers.push("OpenAI");
        }
        if self.brave.is_some() {
            providers.push("Brave Search");
        }
        if providers.is_empty() {
            "None".to_string()
        } else {
            providers.join(", ")
        }
    }
}

/// Embeddings configuration from environment variables
#[derive(Debug, Clone, Default)]
pub struct EmbeddingsConfig {
    /// Custom embedding dimensions (MIRA_EMBEDDING_DIMENSIONS)
    pub dimensions: Option<usize>,
}

impl EmbeddingsConfig {
    /// Load embeddings configuration from environment variables
    pub fn from_env() -> Self {
        let dimensions = std::env::var("MIRA_EMBEDDING_DIMENSIONS")
            .ok()
            .and_then(|d| d.parse().ok());

        if let Some(dims) = dimensions {
            debug!(dimensions = dims, "Custom embedding dimensions configured");
        }

        Self { dimensions }
    }
}

/// Configuration validation result
#[derive(Debug)]
pub struct ConfigValidation {
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl Default for ConfigValidation {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigValidation {
    pub fn new() -> Self {
        Self {
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn add_warning(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    pub fn add_error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }

    /// Format as a human-readable report
    pub fn report(&self) -> String {
        let mut lines = Vec::new();

        if !self.errors.is_empty() {
            lines.push("Errors:".to_string());
            for err in &self.errors {
                lines.push(format!("  - {}", err));
            }
        }

        if !self.warnings.is_empty() {
            lines.push("Warnings:".to_string());
            for warn in &self.warnings {
                lines.push(format!("  - {}", warn));
            }
        }

        if lines.is_empty() {
            "Configuration OK".to_string()
        } else {
            lines.join("\n")
        }
    }
}

/// Runtime guardrails for expert agentic loops
#[derive(Debug, Clone)]
pub struct ExpertGuardrails {
    /// Maximum agentic loop iterations (MIRA_EXPERT_MAX_TURNS, default: 100)
    pub max_turns: usize,
    /// Overall expert consultation timeout in seconds (MIRA_EXPERT_TIMEOUT_SECS, default: 600)
    pub timeout_secs: u64,
    /// Individual LLM call timeout in seconds (MIRA_LLM_CALL_TIMEOUT_SECS, default: 360)
    pub llm_call_timeout_secs: u64,
    /// Maximum concurrent expert consultations (MIRA_MAX_CONCURRENT_EXPERTS, default: 3)
    pub max_concurrent_experts: usize,
    /// Maximum characters per tool result before truncation (MIRA_TOOL_RESULT_MAX_CHARS, default: 16000)
    pub tool_result_max_chars: usize,
    /// MCP tool call timeout in seconds (MIRA_MCP_TOOL_TIMEOUT_SECS, default: 60)
    pub mcp_tool_timeout_secs: u64,
    /// Maximum parallel tool calls per iteration (MIRA_MAX_PARALLEL_TOOL_CALLS, default: 8)
    pub max_parallel_tool_calls: usize,
    /// Maximum total tool calls across all iterations (MIRA_MAX_TOTAL_TOOL_CALLS, default: 200)
    pub max_total_tool_calls: usize,
}

impl Default for ExpertGuardrails {
    fn default() -> Self {
        Self {
            max_turns: 100,
            timeout_secs: 600,
            llm_call_timeout_secs: 360,
            max_concurrent_experts: 3,
            tool_result_max_chars: 16_000,
            mcp_tool_timeout_secs: 60,
            max_parallel_tool_calls: 8,
            max_total_tool_calls: 200,
        }
    }
}

impl ExpertGuardrails {
    /// Load from environment variables, clamped to safe ceilings
    pub fn from_env() -> Self {
        Self {
            max_turns: parse_usize_env("MIRA_EXPERT_MAX_TURNS", 100, 500),
            timeout_secs: parse_u64_env("MIRA_EXPERT_TIMEOUT_SECS", 600, 1800),
            llm_call_timeout_secs: parse_u64_env("MIRA_LLM_CALL_TIMEOUT_SECS", 360, 600),
            max_concurrent_experts: parse_usize_env("MIRA_MAX_CONCURRENT_EXPERTS", 3, 10),
            tool_result_max_chars: parse_usize_env("MIRA_TOOL_RESULT_MAX_CHARS", 16_000, 64_000),
            mcp_tool_timeout_secs: parse_u64_env("MIRA_MCP_TOOL_TIMEOUT_SECS", 60, 300),
            max_parallel_tool_calls: parse_usize_env("MIRA_MAX_PARALLEL_TOOL_CALLS", 8, 20),
            max_total_tool_calls: parse_usize_env("MIRA_MAX_TOTAL_TOOL_CALLS", 200, 1000),
        }
    }
}

/// Environment configuration - all env vars in one place
#[derive(Debug, Clone)]
pub struct EnvConfig {
    /// API keys for LLM providers
    pub api_keys: ApiKeys,
    /// Embeddings configuration
    pub embeddings: EmbeddingsConfig,
    /// Default LLM provider override (DEFAULT_LLM_PROVIDER)
    pub default_provider: Option<String>,
    /// User identity override (MIRA_USER_ID)
    pub user_id: Option<String>,
    /// Enable fuzzy fallback search when embeddings are unavailable (MIRA_FUZZY_FALLBACK)
    pub fuzzy_fallback: bool,
    /// Expert agentic loop guardrails
    pub expert: ExpertGuardrails,
}

impl EnvConfig {
    /// Load all environment configuration (call once at startup)
    pub fn load() -> Self {
        info!("Loading environment configuration");

        Self {
            api_keys: ApiKeys::from_env(),
            embeddings: EmbeddingsConfig::from_env(),
            default_provider: std::env::var("DEFAULT_LLM_PROVIDER")
                .ok()
                .filter(|s| !s.is_empty()),
            user_id: std::env::var("MIRA_USER_ID").ok().filter(|s| !s.is_empty()),
            fuzzy_fallback: parse_bool_env("MIRA_FUZZY_FALLBACK").unwrap_or(true),
            expert: ExpertGuardrails::from_env(),
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> ConfigValidation {
        let mut validation = ConfigValidation::new();

        // Check for LLM providers
        if !self.api_keys.has_llm_provider() {
            validation
                .add_warning("No LLM API keys configured. Set DEEPSEEK_API_KEY or GEMINI_API_KEY.");
        }

        // Check for embeddings
        if !self.api_keys.has_embeddings() {
            validation.add_warning(
                "No embeddings API key configured. Set OPENAI_API_KEY for semantic search.",
            );
        }

        // Validate default provider if set
        if let Some(ref provider) = self.default_provider {
            let valid_providers = ["deepseek", "gemini"];
            if !valid_providers.contains(&provider.to_lowercase().as_str()) {
                validation.add_warning(format!(
                    "Unknown DEFAULT_LLM_PROVIDER '{}'. Valid options: deepseek, gemini",
                    provider
                ));
            }
        }

        validation
    }
}

fn parse_usize_env(name: &str, default: usize, ceiling: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.clamp(1, ceiling))
        .unwrap_or(default)
}

fn parse_u64_env(name: &str, default: u64, ceiling: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|v| v.clamp(1, ceiling))
        .unwrap_or(default)
}

fn parse_bool_env(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?.to_lowercase();
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_keys_summary() {
        // Test with default (empty) keys - no env manipulation needed
        let keys = ApiKeys::default();
        assert!(!keys.has_llm_provider());
        assert!(!keys.has_embeddings());
        assert_eq!(keys.summary(), "None");
    }

    #[test]
    fn test_api_keys_with_values() {
        let keys = ApiKeys {
            deepseek: Some("test-key".to_string()),
            gemini: None,
            openai: None,
            brave: None,
        };
        assert!(keys.has_llm_provider());
        assert!(!keys.has_embeddings());
        assert_eq!(keys.summary(), "DeepSeek");
    }

    #[test]
    fn test_embeddings_config_default() {
        let config = EmbeddingsConfig::default();
        assert!(config.dimensions.is_none());
    }

    #[test]
    fn test_validation_no_keys() {
        let config = EnvConfig {
            api_keys: ApiKeys::default(),
            embeddings: EmbeddingsConfig::default(),
            default_provider: None,
            user_id: None,
            fuzzy_fallback: true,
            expert: ExpertGuardrails::default(),
        };

        let validation = config.validate();
        assert!(validation.is_valid()); // Warnings don't make it invalid
        assert!(!validation.warnings.is_empty());
    }
}
