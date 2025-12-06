// src/llm/router/config.rs
// Configuration for model router

use serde::{Deserialize, Serialize};
use std::env;

/// Configuration for the model router
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Whether routing is enabled (false = use single model)
    pub enabled: bool,

    /// Token threshold for upgrading to Code tier
    pub code_token_threshold: i64,

    /// File count threshold for upgrading to Code tier
    pub code_file_threshold: usize,

    /// Model name for Fast tier (default: gpt-5.1-mini)
    pub fast_model: String,

    /// Model name for Voice tier (default: gpt-5.1)
    pub voice_model: String,

    /// Model name for Code tier (default: gpt-5.1-codex-max)
    pub code_model: String,

    /// Model name for Agentic tier (default: gpt-5.1-codex-max)
    pub agentic_model: String,

    /// Whether to enable fallback on provider failure
    pub enable_fallback: bool,

    /// Log routing decisions
    pub log_routing: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            code_token_threshold: 50_000,
            code_file_threshold: 3,
            fast_model: "gpt-5.1-mini".to_string(),
            voice_model: "gpt-5.1".to_string(),
            code_model: "gpt-5.1-codex-max".to_string(),
            agentic_model: "gpt-5.1-codex-max".to_string(),
            enable_fallback: true,
            log_routing: true,
        }
    }
}

impl RouterConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Core settings
        if let Ok(val) = env::var("MODEL_ROUTER_ENABLED") {
            config.enabled = val.to_lowercase() == "true" || val == "1";
        }

        // Thresholds
        if let Ok(val) = env::var("ROUTE_CODE_TOKEN_THRESHOLD") {
            if let Ok(threshold) = val.parse() {
                config.code_token_threshold = threshold;
            }
        }

        if let Ok(val) = env::var("ROUTE_CODE_FILE_COUNT") {
            if let Ok(count) = val.parse() {
                config.code_file_threshold = count;
            }
        }

        // Model names
        if let Ok(model) = env::var("MODEL_FAST") {
            config.fast_model = model;
        }

        if let Ok(model) = env::var("MODEL_VOICE") {
            config.voice_model = model;
        }

        if let Ok(model) = env::var("MODEL_CODE") {
            config.code_model = model;
        }

        if let Ok(model) = env::var("MODEL_AGENTIC") {
            config.agentic_model = model;
        }

        // Options
        if let Ok(val) = env::var("MODEL_ROUTER_FALLBACK") {
            config.enable_fallback = val.to_lowercase() == "true" || val == "1";
        }

        if let Ok(val) = env::var("MODEL_ROUTER_LOG") {
            config.log_routing = val.to_lowercase() == "true" || val == "1";
        }

        config
    }

    /// Check if router is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RouterConfig::default();

        assert!(config.enabled);
        assert_eq!(config.code_token_threshold, 50_000);
        assert_eq!(config.code_file_threshold, 3);
        assert_eq!(config.fast_model, "gpt-5.1-mini");
        assert_eq!(config.voice_model, "gpt-5.1");
        assert_eq!(config.code_model, "gpt-5.1-codex-max");
        assert_eq!(config.agentic_model, "gpt-5.1-codex-max");
        assert!(config.enable_fallback);
        assert!(config.log_routing);
    }

    #[test]
    fn test_from_env_with_overrides() {
        // Set test environment variables
        // SAFETY: Tests run serially and we clean up after
        unsafe {
            env::set_var("MODEL_ROUTER_ENABLED", "false");
            env::set_var("ROUTE_CODE_TOKEN_THRESHOLD", "100000");
        }

        let config = RouterConfig::from_env();

        assert!(!config.enabled);
        assert_eq!(config.code_token_threshold, 100_000);

        // Clean up
        // SAFETY: Tests run serially
        unsafe {
            env::remove_var("MODEL_ROUTER_ENABLED");
            env::remove_var("ROUTE_CODE_TOKEN_THRESHOLD");
        }
    }
}
