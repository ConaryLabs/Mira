// src/services/chat/config.rs
// Extracted Configuration Management from chat.rs
// Updated to use centralized CONFIG from src/config/mod.rs

use serde::{Deserialize, Serialize};
use crate::config::CONFIG;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConfig {
    model: String,
    verbosity: String,
    reasoning_effort: String,
    max_output_tokens: usize,
    history_message_cap: usize,
    history_token_limit: usize,
    max_retrieval_tokens: usize,
    max_vector_search_results: usize,
    enable_vector_search: bool,
    enable_web_search: bool,
    enable_code_interpreter: bool,
}

impl Default for ChatConfig {
    fn default() -> Self {
        ChatConfig {
            model: CONFIG.model.clone(),
            verbosity: CONFIG.verbosity.clone(),
            reasoning_effort: CONFIG.reasoning_effort.clone(),
            max_output_tokens: CONFIG.max_output_tokens,
            history_message_cap: CONFIG.history_message_cap,
            history_token_limit: CONFIG.history_token_limit,
            max_retrieval_tokens: CONFIG.max_retrieval_tokens,
            max_vector_search_results: CONFIG.max_vector_results,
            enable_vector_search: CONFIG.enable_vector_search,
            enable_web_search: CONFIG.enable_web_search,
            enable_code_interpreter: CONFIG.enable_code_interpreter,
        }
    }
}

impl ChatConfig {
    /// Create new configuration with custom values
    pub fn new(
        model: String,
        verbosity: String,
        reasoning_effort: String,
        max_output_tokens: usize,
        history_message_cap: usize,
        history_token_limit: usize,
        max_retrieval_tokens: usize,
        max_vector_search_results: usize,
        enable_vector_search: bool,
        enable_web_search: bool,
        enable_code_interpreter: bool,
    ) -> Self {
        Self {
            model,
            verbosity,
            reasoning_effort,
            max_output_tokens,
            history_message_cap,
            history_token_limit,
            max_retrieval_tokens,
            max_vector_search_results,
            enable_vector_search,
            enable_web_search,
            enable_code_interpreter,
        }
    }

    /// Create configuration from centralized CONFIG with optional overrides
    pub fn from_config_with_overrides(
        model_override: Option<String>,
        history_cap_override: Option<usize>,
        vector_search_override: Option<bool>,
    ) -> Self {
        Self {
            model: model_override.unwrap_or_else(|| CONFIG.model.clone()),
            verbosity: CONFIG.verbosity.clone(),
            reasoning_effort: CONFIG.reasoning_effort.clone(),
            max_output_tokens: CONFIG.max_output_tokens,
            history_message_cap: history_cap_override.unwrap_or(CONFIG.history_message_cap),
            history_token_limit: CONFIG.history_token_limit,
            max_retrieval_tokens: CONFIG.max_retrieval_tokens,
            max_vector_search_results: CONFIG.max_vector_results,
            enable_vector_search: vector_search_override.unwrap_or(CONFIG.enable_vector_search),
            enable_web_search: CONFIG.enable_web_search,
            enable_code_interpreter: CONFIG.enable_code_interpreter,
        }
    }

    // Getters
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn verbosity(&self) -> &str {
        &self.verbosity
    }

    pub fn reasoning_effort(&self) -> &str {
        &self.reasoning_effort
    }

    pub fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }

    pub fn history_message_cap(&self) -> usize {
        self.history_message_cap
    }

    pub fn history_token_limit(&self) -> usize {
        self.history_token_limit
    }

    pub fn max_retrieval_tokens(&self) -> usize {
        self.max_retrieval_tokens
    }

    pub fn max_vector_search_results(&self) -> usize {
        self.max_vector_search_results
    }

    pub fn enable_vector_search(&self) -> bool {
        self.enable_vector_search
    }

    pub fn enable_web_search(&self) -> bool {
        self.enable_web_search
    }

    pub fn enable_code_interpreter(&self) -> bool {
        self.enable_code_interpreter
    }

    // Setters for dynamic configuration
    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }

    pub fn with_history_cap(mut self, cap: usize) -> Self {
        self.history_message_cap = cap;
        self
    }

    pub fn with_vector_search(mut self, enabled: bool) -> Self {
        self.enable_vector_search = enabled;
        self
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), String> {
        if self.model.is_empty() {
            return Err("Model name cannot be empty".to_string());
        }

        if self.max_output_tokens == 0 {
            return Err("Max output tokens must be greater than 0".to_string());
        }

        if self.history_message_cap == 0 {
            return Err("History message cap must be greater than 0".to_string());
        }

        if self.max_vector_search_results == 0 && self.enable_vector_search {
            return Err("Vector search results must be greater than 0 when enabled".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ChatConfig::default();
        assert!(!config.model().is_empty());
        assert!(config.max_output_tokens() > 0);
        assert!(config.history_message_cap() > 0);
    }

    #[test]
    fn test_config_validation() {
        let mut config = ChatConfig::default();
        assert!(config.validate().is_ok());

        // Test invalid model
        config.model = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_builders() {
        let config = ChatConfig::default()
            .with_model("gpt-4".to_string())
            .with_history_cap(50)
            .with_vector_search(true);

        assert_eq!(config.model(), "gpt-4");
        assert_eq!(config.history_message_cap(), 50);
        assert!(config.enable_vector_search());
    }
}
