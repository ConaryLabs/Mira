// crates/mira-server/src/config/env.rs
// Environment-based configuration - single source of truth for all env vars

use tracing::{debug, info, warn};

/// API keys loaded from environment variables
#[derive(Clone, Default)]
pub struct ApiKeys {
    /// DeepSeek API key (DEEPSEEK_API_KEY)
    pub deepseek: Option<String>,
    /// Zhipu API key (ZHIPU_API_KEY) — GLM-5 via coding endpoint
    pub zhipu: Option<String>,
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
            .field("zhipu", &redact(&self.zhipu))
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
                zhipu: None,
                ollama: None,
                openai: None,
                brave: Self::read_key("BRAVE_API_KEY"),
            };
        }

        let deepseek = Self::read_key("DEEPSEEK_API_KEY");
        let zhipu = Self::read_key("ZHIPU_API_KEY");
        let ollama = Self::read_key("OLLAMA_HOST");
        let openai = Self::read_key("OPENAI_API_KEY");
        let brave = Self::read_key("BRAVE_API_KEY");

        let keys = Self {
            deepseek,
            zhipu,
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
        if self.zhipu.is_some() {
            available.push("Zhipu GLM");
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
        self.deepseek.is_some() || self.zhipu.is_some() || self.ollama.is_some()
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
        if self.zhipu.is_some() {
            providers.push("Zhipu GLM");
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
}

impl EmbeddingsConfig {
    /// Load embeddings configuration from environment variables
    pub fn from_env() -> Self {
        let dimensions = std::env::var("MIRA_EMBEDDING_DIMENSIONS")
            .ok()
            .and_then(|d| d.parse::<usize>().ok());

        let dimensions = if let Some(dims) = dimensions {
            if dims != 1536 {
                warn!(
                    configured = dims,
                    expected = 1536,
                    "MIRA_EMBEDDING_DIMENSIONS={} does not match vec_memory table (1536). Ignoring custom value.",
                    dims
                );
                None
            } else {
                debug!(dimensions = dims, "Custom embedding dimensions configured");
                Some(dims)
            }
        } else {
            None
        };

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
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> ConfigValidation {
        let mut validation = ConfigValidation::new();

        // Check for LLM providers
        if !self.api_keys.has_llm_provider() {
            validation.add_warning(
                "No LLM API keys configured. Set DEEPSEEK_API_KEY, ZHIPU_API_KEY, or OLLAMA_HOST.",
            );
        }

        // Check for embeddings
        if !self.api_keys.has_embeddings() {
            validation.add_warning(
                "No embeddings API key configured. Set OPENAI_API_KEY for semantic search.",
            );
        }

        // Validate default provider if set
        if let Some(ref provider) = self.default_provider {
            let valid_providers = ["deepseek", "zhipu", "ollama"];
            if !valid_providers.contains(&provider.to_lowercase().as_str()) {
                validation.add_warning(format!(
                    "Unknown DEFAULT_LLM_PROVIDER '{}'. Valid options: deepseek, zhipu, ollama",
                    provider
                ));
            }
        }

        validation
    }
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
    fn test_api_keys_with_ollama() {
        let keys = ApiKeys {
            deepseek: None,
            zhipu: None,
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
            zhipu: None,
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
            fuzzy_fallback: true,
        };

        let validation = config.validate();
        assert!(validation.is_valid()); // Warnings don't make it invalid
        assert!(!validation.warnings.is_empty());
    }
}
