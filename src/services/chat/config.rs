// src/services/chat/config.rs
// PHASE 0: MINIMAL GPT-5 Robust Memory configuration support to ChatConfig
// Extracted Configuration Management from chat.rs
// Updated to use centralized CONFIG from src/config/mod.rs

use serde::{Deserialize, Serialize};
use crate::config::CONFIG;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConfig {
    // ── Core LLM Configuration ──
    model: String,
    verbosity: String,
    reasoning_effort: String,
    max_output_tokens: usize,
    
    // ── History & Memory Configuration ──
    history_message_cap: usize,
    history_token_limit: usize,
    max_retrieval_tokens: usize,
    max_vector_search_results: usize,
    
    // ── Feature Flags ──
    enable_vector_search: bool,
    enable_web_search: bool,
    enable_code_interpreter: bool,

    // ── PHASE 0: MINIMAL Robust Memory Configuration ──
    /// Whether this chat instance should use robust memory features
    enable_robust_memory: bool,
    
    /// List of embedding heads to use for this chat session
    embedding_heads: Vec<String>,
    
    /// Enable rolling summaries for this chat session
    enable_rolling_summaries: bool,
}

impl Default for ChatConfig {
    fn default() -> Self {
        // Get embedding heads from global config, but fallback if robust memory disabled
        let embedding_heads = if CONFIG.is_robust_memory_enabled() {
            CONFIG.get_embedding_heads()
        } else {
            vec!["semantic".to_string()]
        };

        ChatConfig {
            // ── Core LLM Configuration ──
            model: CONFIG.model.clone(),
            verbosity: CONFIG.verbosity.clone(),
            reasoning_effort: CONFIG.reasoning_effort.clone(),
            max_output_tokens: CONFIG.max_output_tokens,
            
            // ── History & Memory Configuration ──
            history_message_cap: CONFIG.history_message_cap,
            history_token_limit: CONFIG.history_token_limit,
            max_retrieval_tokens: CONFIG.max_retrieval_tokens,
            max_vector_search_results: CONFIG.max_vector_results,
            
            // ── Feature Flags ──
            enable_vector_search: CONFIG.enable_vector_search,
            enable_web_search: CONFIG.enable_web_search,
            enable_code_interpreter: CONFIG.enable_code_interpreter,

            // ── PHASE 0: Minimal Robust Memory Configuration ──
            enable_robust_memory: CONFIG.is_robust_memory_enabled(),
            embedding_heads,
            enable_rolling_summaries: CONFIG.rolling_summaries_enabled(),
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
            // Use defaults for Phase 0 fields
            ..Default::default()
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
            history_message_cap: history_cap_override.unwrap_or(CONFIG.history_message_cap),
            enable_vector_search: vector_search_override.unwrap_or(CONFIG.enable_vector_search),
            // Use defaults for other fields
            ..Default::default()
        }
    }

    // ── PHASE 0: Minimal Builder Methods for Robust Memory ──
    
    /// Enable or disable robust memory features for this chat session
    pub fn with_robust_memory(mut self, enabled: bool) -> Self {
        self.enable_robust_memory = enabled;
        
        // If disabling robust memory, fallback to single head
        if !enabled {
            self.embedding_heads = vec!["semantic".to_string()];
            self.enable_rolling_summaries = false;
        }
        self
    }
    
    /// Set specific embedding heads to use
    pub fn with_embedding_heads(mut self, heads: Vec<String>) -> Self {
        if self.enable_robust_memory {
            self.embedding_heads = heads;
        }
        self
    }
    
    /// Enable or disable rolling summaries
    pub fn with_rolling_summaries(mut self, enabled: bool) -> Self {
        if self.enable_robust_memory {
            self.enable_rolling_summaries = enabled;
        }
        self
    }

    // ── Existing Getters ──
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

    // ── PHASE 0: Minimal Getters for Robust Memory ──
    
    pub fn enable_robust_memory(&self) -> bool {
        self.enable_robust_memory
    }
    
    pub fn embedding_heads(&self) -> &[String] {
        &self.embedding_heads
    }
    
    pub fn enable_rolling_summaries(&self) -> bool {
        self.enable_rolling_summaries
    }

    // ── Existing Builder Methods ──
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

    // ── PHASE 0: Minimal Validation ──
    
    /// Validate configuration values including basic Phase 0 fields
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

        // ── Phase 0 Validations ──
        
        if self.enable_robust_memory {
            if self.embedding_heads.is_empty() {
                return Err("At least one embedding head must be specified when robust memory is enabled".to_string());
            }
            
            for head in &self.embedding_heads {
                if !["semantic", "code", "summary"].contains(&head.as_str()) {
                    return Err(format!("Invalid embedding head '{}'. Must be one of: semantic, code, summary", head));
                }
            }
        }

        Ok(())
    }

    // ── PHASE 0: Simple Analysis Methods ──
    
    /// Check if this configuration uses any advanced features
    pub fn uses_advanced_features(&self) -> bool {
        self.enable_robust_memory
    }
    
    /// Get a summary description of this configuration
    pub fn get_description(&self) -> String {
        if self.enable_robust_memory {
            format!(
                "ChatConfig: {} | Robust memory with {} heads | Rolling summaries: {}",
                self.model,
                self.embedding_heads.len(),
                if self.enable_rolling_summaries { "enabled" } else { "disabled" }
            )
        } else {
            format!("ChatConfig: {} | Standard memory", self.model)
        }
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
        
        // Test Phase 0 defaults - should match global config
        assert_eq!(config.enable_robust_memory(), CONFIG.is_robust_memory_enabled());
        assert!(!config.embedding_heads().is_empty());
    }

    #[test]
    fn test_config_validation() {
        let mut config = ChatConfig::default();
        assert!(config.validate().is_ok());

        // Test invalid model
        config.model = String::new();
        assert!(config.validate().is_err());
        
        // Reset model for further tests
        config.model = "gpt-5".to_string();
        
        // Test Phase 0 validations
        config.enable_robust_memory = true;
        config.embedding_heads = vec![]; // Invalid: empty heads
        assert!(config.validate().is_err());
        
        config.embedding_heads = vec!["invalid_head".to_string()]; // Invalid head name
        assert!(config.validate().is_err());
        
        config.embedding_heads = vec!["semantic".to_string(), "code".to_string()]; // Valid heads
        assert!(config.validate().is_ok());
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

    #[test]
    fn test_phase0_robust_memory_builder() {
        let config = ChatConfig::default()
            .with_robust_memory(true)
            .with_embedding_heads(vec!["semantic".to_string(), "code".to_string()])
            .with_rolling_summaries(true);

        assert!(config.enable_robust_memory());
        assert_eq!(config.embedding_heads().len(), 2);
        assert!(config.enable_rolling_summaries());
        
        // Test description generation
        let description = config.get_description();
        assert!(description.contains("Robust memory"));
        assert!(description.contains("2 heads"));
    }

    #[test]
    fn test_phase0_robust_memory_disable() {
        let config = ChatConfig::default()
            .with_robust_memory(true)
            .with_embedding_heads(vec!["semantic".to_string(), "code".to_string()])
            .with_rolling_summaries(true)
            .with_robust_memory(false); // Disable after enabling

        // Should fallback to single head and disable advanced features
        assert!(!config.enable_robust_memory());
        assert_eq!(config.embedding_heads(), &["semantic"]);
        assert!(!config.enable_rolling_summaries());
    }

    #[test]
    fn test_advanced_features_detection() {
        let basic_config = ChatConfig::default();
        assert!(!basic_config.uses_advanced_features());

        let advanced_config = ChatConfig::default()
            .with_robust_memory(true);
        assert!(advanced_config.uses_advanced_features());
    }
}
