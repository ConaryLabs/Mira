// src/llm/router/config.rs
// Configuration for model router

use serde::{Deserialize, Serialize};
use std::env;

/// Configuration for the model router
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Whether routing is enabled (false = use single model)
    pub enabled: bool,

    /// Token threshold for upgrading to Thinker tier
    pub thinker_token_threshold: i64,

    /// File count threshold for upgrading to Thinker tier
    pub thinker_file_threshold: usize,

    /// Model name for Fast tier (default: gpt-5.1-mini)
    pub fast_model: String,

    /// Model name for Voice tier (default: gpt-5.1)
    pub voice_model: String,

    /// Model name for Thinker tier (default: gpt-5.1 with high reasoning)
    pub thinker_model: String,

    /// Whether to enable fallback on provider failure
    pub enable_fallback: bool,

    /// Log routing decisions
    pub log_routing: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            thinker_token_threshold: 50_000,
            thinker_file_threshold: 3,
            fast_model: "gpt-5.1-mini".to_string(),
            voice_model: "gpt-5.1".to_string(),
            thinker_model: "gpt-5.1".to_string(),
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
        if let Ok(val) = env::var("ROUTE_THINKER_TOKEN_THRESHOLD") {
            if let Ok(threshold) = val.parse() {
                config.thinker_token_threshold = threshold;
            }
        }

        if let Ok(val) = env::var("ROUTE_THINKER_FILE_COUNT") {
            if let Ok(count) = val.parse() {
                config.thinker_file_threshold = count;
            }
        }

        // Model names
        if let Ok(model) = env::var("MODEL_FAST") {
            config.fast_model = model;
        }

        if let Ok(model) = env::var("MODEL_VOICE") {
            config.voice_model = model;
        }

        if let Ok(model) = env::var("MODEL_THINKER") {
            config.thinker_model = model;
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
        assert_eq!(config.thinker_token_threshold, 50_000);
        assert_eq!(config.thinker_file_threshold, 3);
        assert_eq!(config.fast_model, "gpt-5.1-mini");
        assert_eq!(config.voice_model, "gpt-5.1");
        assert_eq!(config.thinker_model, "gpt-5.1");
        assert!(config.enable_fallback);
        assert!(config.log_routing);
    }

    #[test]
    fn test_from_env_with_overrides() {
        // Set test environment variables
        // SAFETY: Tests run serially and we clean up after
        unsafe {
            env::set_var("MODEL_ROUTER_ENABLED", "false");
            env::set_var("ROUTE_THINKER_TOKEN_THRESHOLD", "100000");
        }

        let config = RouterConfig::from_env();

        assert!(!config.enabled);
        assert_eq!(config.thinker_token_threshold, 100_000);

        // Clean up
        // SAFETY: Tests run serially
        unsafe {
            env::remove_var("MODEL_ROUTER_ENABLED");
            env::remove_var("ROUTE_THINKER_TOKEN_THRESHOLD");
        }
    }
}
