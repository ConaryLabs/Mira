// crates/mira-server/src/config/env.rs
// Environment-based configuration - single source of truth for all env vars

use crate::llm::Provider;
use tracing::{debug, info, warn};

/// API keys loaded from environment variables
#[derive(Clone, Default)]
pub struct ApiKeys {
    /// DeepSeek API key (DEEPSEEK_API_KEY)
    pub deepseek: Option<String>,
    /// Ollama host URL (OLLAMA_HOST) — local LLM, no API key needed
    pub ollama: Option<String>,
    /// OpenAI API key (OPENAI_API_KEY) — used for embeddings
    pub openai: Option<String>,
    /// Brave Search API key (BRAVE_API_KEY)
    pub brave: Option<String>,
}

impl std::fmt::Debug for ApiKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn redact(opt: &Option<String>) -> &str {
            match opt {
                Some(_) => "Some(<redacted>)",
                None => "None",
            }
        }
        f.debug_struct("ApiKeys")
            .field("deepseek", &redact(&self.deepseek))
            .field("ollama", &redact(&self.ollama))
            .field("openai", &redact(&self.openai))
            .field("brave", &redact(&self.brave))
            .finish()
    }
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
                ollama: None,
                openai: None,
                brave: Self::read_key("BRAVE_API_KEY"),
            };
        }

        let deepseek = Self::read_key("DEEPSEEK_API_KEY");
        let ollama = Self::read_key("OLLAMA_HOST");
        let openai = Self::read_key("OPENAI_API_KEY");
        let brave = Self::read_key("BRAVE_API_KEY");

        let keys = Self {
            deepseek,
            ollama,
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
        if self.ollama.is_some() {
            available.push("Ollama");
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
        self.deepseek.is_some() || self.ollama.is_some()
    }

    /// Check if embeddings are available (OpenAI key or Ollama host)
    pub fn has_embeddings(&self) -> bool {
        self.openai.is_some() || self.ollama.is_some()
    }

    /// Get a summary of available providers
    pub fn summary(&self) -> String {
        let mut providers = Vec::new();
        if self.deepseek.is_some() {
            providers.push("DeepSeek");
        }
        if self.ollama.is_some() {
            providers.push("Ollama");
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
    /// Ollama embedding model override (OLLAMA_EMBEDDING_MODEL)
    pub ollama_embedding_model: Option<String>,
}

impl EmbeddingsConfig {
    /// Load embeddings configuration from environment variables
    pub fn from_env() -> Self {
        let dimensions = std::env::var("MIRA_EMBEDDING_DIMENSIONS")
            .ok()
            .and_then(|d| d.parse::<usize>().ok());

        if let Some(dims) = dimensions {
            debug!(dimensions = dims, "Custom embedding dimensions configured");
        }

        let ollama_embedding_model = std::env::var("OLLAMA_EMBEDDING_MODEL")
            .ok()
            .filter(|s| !s.trim().is_empty());

        if let Some(ref model) = ollama_embedding_model {
            debug!(model = %model, "Custom Ollama embedding model configured");
        }

        Self {
            dimensions,
            ollama_embedding_model,
        }
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
    /// Enable fuzzy search in hybrid search pipeline (MIRA_FUZZY_SEARCH)
    pub fuzzy_search: bool,
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
            fuzzy_search: parse_bool_env("MIRA_FUZZY_SEARCH").unwrap_or(true),
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> ConfigValidation {
        let mut validation = ConfigValidation::new();

        // Check for LLM providers
        if !self.api_keys.has_llm_provider() {
            validation
                .add_warning("No LLM API keys configured. Set DEEPSEEK_API_KEY or OLLAMA_HOST.");
        }

        // Check for embeddings
        if !self.api_keys.has_embeddings() {
            validation.add_warning(
                "No embeddings provider configured. Set OPENAI_API_KEY or OLLAMA_HOST for semantic search.",
            );
        }

        // Validate default provider if set
        if let Some(ref provider) = self.default_provider {
            match Provider::from_str(provider) {
                None => {
                    validation.add_warning(format!(
                        "Unknown DEFAULT_LLM_PROVIDER '{}'. Valid options: deepseek, ollama",
                        provider
                    ));
                }
                Some(Provider::Sampling) => {
                    validation.add_warning(
                        "DEFAULT_LLM_PROVIDER='sampling' has no effect. \
                         MCP sampling is used automatically as a last-resort fallback."
                            .to_string(),
                    );
                }
                Some(_) => {} // valid
            }
        }

        validation
    }
}

pub(crate) fn parse_bool_env(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?.to_lowercase();
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => {
            warn!(
                env = name,
                value = %value,
                "Unrecognized boolean value for {}. Expected: true/false, 1/0, yes/no, on/off. Treating as unset.",
                name
            );
            None
        }
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
    fn test_api_keys_with_ollama() {
        let keys = ApiKeys {
            deepseek: None,

            ollama: Some("http://localhost:11434".to_string()),
            openai: None,
            brave: None,
        };
        assert!(keys.has_llm_provider());
        assert_eq!(keys.summary(), "Ollama");
    }

    #[test]
    fn test_api_keys_with_values() {
        let keys = ApiKeys {
            deepseek: Some("test-key".to_string()),

            ollama: None,
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
            fuzzy_search: true,
        };

        let validation = config.validate();
        assert!(validation.is_valid()); // Warnings don't make it invalid
        assert!(!validation.warnings.is_empty());
    }

    #[test]
    fn test_validation_invalid_provider() {
        let config = EnvConfig {
            api_keys: ApiKeys::default(),
            embeddings: EmbeddingsConfig::default(),
            default_provider: Some("gpt4".to_string()),
            user_id: None,
            fuzzy_search: true,
        };

        let validation = config.validate();
        assert!(
            validation.warnings.iter().any(|w| w.contains("gpt4")),
            "Should warn about invalid provider"
        );
    }

    #[test]
    fn test_validation_valid_provider() {
        let config = EnvConfig {
            api_keys: ApiKeys {
                deepseek: Some("key".to_string()),
                ..Default::default()
            },
            embeddings: EmbeddingsConfig::default(),
            default_provider: Some("deepseek".to_string()),
            user_id: None,
            fuzzy_search: true,
        };

        let validation = config.validate();
        // Should not warn about provider name
        assert!(
            !validation
                .warnings
                .iter()
                .any(|w| w.contains("Unknown DEFAULT_LLM_PROVIDER")),
        );
    }

    #[test]
    fn test_validation_sampling_provider_warns() {
        let config = EnvConfig {
            api_keys: ApiKeys::default(),
            embeddings: EmbeddingsConfig::default(),
            default_provider: Some("sampling".to_string()),
            user_id: None,
            fuzzy_search: true,
        };

        let validation = config.validate();
        assert!(
            validation
                .warnings
                .iter()
                .any(|w| w.contains("sampling") && w.contains("no effect")),
            "Should warn that sampling has no effect as DEFAULT_LLM_PROVIDER"
        );
    }

    #[test]
    fn test_validation_provider_alias_glm_rejected() {
        let config = EnvConfig {
            api_keys: ApiKeys::default(),
            embeddings: EmbeddingsConfig::default(),
            default_provider: Some("glm".to_string()),
            user_id: None,
            fuzzy_search: true,
        };

        let validation = config.validate();
        // "glm" was a Zhipu alias — now invalid since Zhipu was removed
        assert!(
            validation
                .warnings
                .iter()
                .any(|w| w.contains("Unknown DEFAULT_LLM_PROVIDER")),
        );
    }

    // ============================================================================
    // parse_bool_env tests (C1)
    // ============================================================================

    // Note: parse_bool_env reads from actual env vars. These tests use unique
    // env var names to avoid interference with each other and production vars.

    #[test]
    fn test_parse_bool_env_true_values() {
        for (i, val) in ["1", "true", "True", "TRUE", "yes", "YES", "on", "ON"]
            .iter()
            .enumerate()
        {
            let name = format!("MIRA_TEST_BOOL_TRUE_{}", i);
            // SAFETY: test-only, unique env var names avoid interference
            unsafe {
                std::env::set_var(&name, val);
            }
            assert_eq!(
                parse_bool_env(&name),
                Some(true),
                "Expected true for '{}'",
                val
            );
            unsafe {
                std::env::remove_var(&name);
            }
        }
    }

    #[test]
    fn test_parse_bool_env_false_values() {
        for (i, val) in ["0", "false", "False", "FALSE", "no", "NO", "off", "OFF"]
            .iter()
            .enumerate()
        {
            let name = format!("MIRA_TEST_BOOL_FALSE_{}", i);
            // SAFETY: test-only, unique env var names avoid interference
            unsafe {
                std::env::set_var(&name, val);
            }
            assert_eq!(
                parse_bool_env(&name),
                Some(false),
                "Expected false for '{}'",
                val
            );
            unsafe {
                std::env::remove_var(&name);
            }
        }
    }

    #[test]
    fn test_parse_bool_env_invalid_returns_none() {
        let name = "MIRA_TEST_BOOL_INVALID";
        // SAFETY: test-only, unique env var name
        unsafe {
            std::env::set_var(name, "banana");
        }
        assert_eq!(
            parse_bool_env(name),
            None,
            "Invalid value should return None"
        );
        unsafe {
            std::env::remove_var(name);
        }
    }

    #[test]
    fn test_parse_bool_env_unset_returns_none() {
        assert_eq!(parse_bool_env("MIRA_TEST_BOOL_NONEXISTENT_XYZ"), None);
    }

    // ============================================================================
    // read_key tests
    // ============================================================================

    #[test]
    fn test_read_key_empty_value() {
        // SAFETY: test-only, unique env var name
        unsafe {
            std::env::set_var("MIRA_TEST_EMPTY_KEY", "");
        }
        assert_eq!(ApiKeys::read_key("MIRA_TEST_EMPTY_KEY"), None);
        unsafe {
            std::env::remove_var("MIRA_TEST_EMPTY_KEY");
        }
    }

    #[test]
    fn test_read_key_whitespace_only() {
        // SAFETY: test-only, unique env var name
        unsafe {
            std::env::set_var("MIRA_TEST_WS_KEY", "   ");
        }
        assert_eq!(ApiKeys::read_key("MIRA_TEST_WS_KEY"), None);
        unsafe {
            std::env::remove_var("MIRA_TEST_WS_KEY");
        }
    }

    #[test]
    fn test_read_key_valid() {
        // SAFETY: test-only, unique env var name
        unsafe {
            std::env::set_var("MIRA_TEST_VALID_KEY", "sk-12345");
        }
        assert_eq!(
            ApiKeys::read_key("MIRA_TEST_VALID_KEY"),
            Some("sk-12345".to_string())
        );
        unsafe {
            std::env::remove_var("MIRA_TEST_VALID_KEY");
        }
    }
}
