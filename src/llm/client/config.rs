// src/llm/client/config.rs
// Configuration management for Claude client using centralized CONFIG

use anyhow::Result;
use tracing::debug;
use crate::config::CONFIG;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_output_tokens: usize,
}

impl ClientConfig {
    /// Create configuration from centralized CONFIG
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY must be set"))?;
        
        debug!(
            "Initialized Claude client: model={}, max_tokens={}",
            CONFIG.anthropic_model, CONFIG.anthropic_max_tokens
        );

        Ok(Self {
            api_key,
            base_url: CONFIG.anthropic_base_url.clone(),
            model: CONFIG.anthropic_model.clone(),
            max_output_tokens: CONFIG.anthropic_max_tokens,
        })
    }

    /// Create configuration with custom values (for testing)
    pub fn new(
        api_key: String,
        base_url: String,
        model: String,
        max_output_tokens: usize,
    ) -> Self {
        Self {
            api_key,
            base_url,
            model,
            max_output_tokens,
        }
    }

    /// Get API key for authentication
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Get base URL for API requests
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get model name
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get maximum output tokens
    pub fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<()> {
        if self.api_key.is_empty() {
            return Err(anyhow::anyhow!("API key cannot be empty"));
        }

        if self.base_url.is_empty() {
            return Err(anyhow::anyhow!("Base URL cannot be empty"));
        }

        // Validate max tokens range (Claude supports up to 200K)
        if self.max_output_tokens == 0 || self.max_output_tokens > 200000 {
            return Err(anyhow::anyhow!("max_output_tokens must be between 1 and 200000"));
        }

        Ok(())
    }

    /// Get default headers for Claude API requests
    pub fn default_headers(&self) -> Vec<(String, String)> {
        vec![
            ("x-api-key".to_string(), self.api_key.clone()),
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ("content-type".to_string(), "application/json".to_string()),
            ("accept".to_string(), "application/json".to_string()),
        ]
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            api_key: "".to_string(),
            base_url: CONFIG.anthropic_base_url.clone(),
            model: CONFIG.anthropic_model.clone(),
            max_output_tokens: CONFIG.anthropic_max_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub model: String,
    pub max_output_tokens: usize,
}

impl ModelConfig {
    /// Create from centralized CONFIG
    pub fn from_config() -> Self {
        Self {
            model: CONFIG.anthropic_model.clone(),
            max_output_tokens: CONFIG.anthropic_max_tokens,
        }
    }

    /// Check if model supports streaming
    pub fn supports_streaming(&self) -> bool {
        self.model.starts_with("claude-")
    }

    /// Get recommended timeout based on thinking budget
    pub fn recommended_timeout_secs(&self) -> u64 {
        // Claude with extended thinking can take longer
        CONFIG.openai_timeout  // Reuse the timeout config
    }

    /// Convert to JSON for API requests
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_output_tokens
        })
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self::from_config()
    }
}
