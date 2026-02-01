// crates/mira-server/src/config/env.rs
// Environment-based configuration - single source of truth for all env vars

use crate::embeddings::TaskType;
use tracing::{debug, info, warn};

/// API keys loaded from environment variables
#[derive(Debug, Clone, Default)]
pub struct ApiKeys {
    /// DeepSeek API key (DEEPSEEK_API_KEY)
    pub deepseek: Option<String>,
    /// Gemini/Google API key (GEMINI_API_KEY or GOOGLE_API_KEY)
    pub gemini: Option<String>,
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
                brave: Self::read_key("BRAVE_API_KEY"),
            };
        }

        let deepseek = Self::read_key("DEEPSEEK_API_KEY");
        let gemini = Self::read_key("GEMINI_API_KEY").or_else(|| Self::read_key("GOOGLE_API_KEY"));
        let brave = Self::read_key("BRAVE_API_KEY");

        let keys = Self {
            deepseek,
            gemini,
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

    /// Check if embeddings are available (requires Gemini key)
    pub fn has_embeddings(&self) -> bool {
        self.gemini.is_some()
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
    /// Task type for embeddings (MIRA_EMBEDDING_TASK_TYPE)
    pub task_type: TaskType,
}

impl EmbeddingsConfig {
    /// Load embeddings configuration from environment variables
    pub fn from_env() -> Self {
        let dimensions = std::env::var("MIRA_EMBEDDING_DIMENSIONS")
            .ok()
            .and_then(|d| d.parse().ok());

        let task_type = std::env::var("MIRA_EMBEDDING_TASK_TYPE")
            .ok()
            .and_then(|t| Self::parse_task_type(&t))
            .unwrap_or_default();

        if let Some(dims) = dimensions {
            debug!(dimensions = dims, "Custom embedding dimensions configured");
        }

        Self {
            dimensions,
            task_type,
        }
    }

    /// Parse task type from string
    fn parse_task_type(s: &str) -> Option<TaskType> {
        match s.to_uppercase().as_str() {
            "SEMANTIC_SIMILARITY" => Some(TaskType::SemanticSimilarity),
            "RETRIEVAL_DOCUMENT" => Some(TaskType::RetrievalDocument),
            "RETRIEVAL_QUERY" => Some(TaskType::RetrievalQuery),
            "CLASSIFICATION" => Some(TaskType::Classification),
            "CLUSTERING" => Some(TaskType::Clustering),
            "CODE_RETRIEVAL_QUERY" => Some(TaskType::CodeRetrievalQuery),
            "QUESTION_ANSWERING" => Some(TaskType::QuestionAnswering),
            "FACT_VERIFICATION" => Some(TaskType::FactVerification),
            _ => {
                warn!(value = s, "Unknown MIRA_EMBEDDING_TASK_TYPE, using default");
                None
            }
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
            validation
                .add_warning("No LLM API keys configured. Set DEEPSEEK_API_KEY or GEMINI_API_KEY.");
        }

        // Check for embeddings
        if !self.api_keys.has_embeddings() {
            validation.add_warning(
                "No embeddings API key configured. Set GEMINI_API_KEY for semantic search.",
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
