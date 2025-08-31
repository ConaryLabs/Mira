// src/llm/client/config.rs
// Phase 1: Extract Configuration Management from client.rs
// Updated to use centralized CONFIG from src/config/mod.rs
// CLEANED: Removed emojis for professional, terminal-friendly logging

use anyhow::Result;
use tracing::{debug};
use crate::config::CONFIG;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
}

impl ClientConfig {
    /// Create configuration from centralized CONFIG and environment variables
    pub fn from_env() -> Result<Self> {
        // API key still needs to come from env as it's sensitive
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY must be set"))?;
        
        // Base URL from env (fallback to default)
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_string());

        debug!(
            "Initialized LLM client config: model={}, verbosity={}, reasoning={}, max_tokens={}",
            CONFIG.model, CONFIG.verbosity, CONFIG.reasoning_effort, CONFIG.max_output_tokens
        );

        Ok(Self {
            api_key,
            base_url,
            model: CONFIG.model.clone(),
            verbosity: CONFIG.verbosity.clone(),
            reasoning_effort: CONFIG.reasoning_effort.clone(),
            max_output_tokens: CONFIG.max_output_tokens,
        })
    }

    /// Create configuration with custom values (for testing)
    pub fn new(
        api_key: String,
        base_url: String,
        model: String,
        verbosity: String,
        reasoning_effort: String,
        max_output_tokens: usize,
    ) -> Self {
        Self {
            api_key,
            base_url,
            model,
            verbosity,
            reasoning_effort,
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

    /// Get verbosity setting
    pub fn verbosity(&self) -> &str {
        &self.verbosity
    }

    /// Get reasoning effort setting
    pub fn reasoning_effort(&self) -> &str {
        &self.reasoning_effort
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

        // Validate verbosity levels
        match self.verbosity.as_str() {
            "low" | "medium" | "high" => {},
            _ => return Err(anyhow::anyhow!("Invalid verbosity level. Must be 'low', 'medium', or 'high'")),
        }

        // Validate reasoning effort levels
        match self.reasoning_effort.as_str() {
            "low" | "medium" | "high" => {},
            _ => return Err(anyhow::anyhow!("Invalid reasoning effort level. Must be 'low', 'medium', or 'high'")),
        }

        // Validate max tokens range
        if self.max_output_tokens == 0 || self.max_output_tokens > 200000 {
            return Err(anyhow::anyhow!("Max output tokens must be between 1 and 200000"));
        }

        Ok(())
    }

    /// Get default headers for HTTP requests
    pub fn default_headers(&self) -> Vec<(String, String)> {
        vec![
            ("authorization".to_string(), format!("Bearer {}", self.api_key)),
            ("content-type".to_string(), "application/json".to_string()),
            ("accept".to_string(), "application/json".to_string()),
        ]
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            api_key: "".to_string(),
            base_url: "https://api.openai.com".to_string(),
            model: "gpt-5".to_string(),
            verbosity: "high".to_string(),
            reasoning_effort: "high".to_string(),
            max_output_tokens: 128000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
}

impl ModelConfig {
    /// Create from centralized CONFIG
    pub fn from_config() -> Self {
        Self {
            model: CONFIG.model.clone(),
            verbosity: CONFIG.verbosity.clone(),
            reasoning_effort: CONFIG.reasoning_effort.clone(),
            max_output_tokens: CONFIG.max_output_tokens,
        }
    }

    /// Check if model supports streaming
    pub fn supports_streaming(&self) -> bool {
        // GPT-5 and similar models support streaming
        self.model.starts_with("gpt-") || self.model.starts_with("claude-")
    }

    /// Get recommended timeout based on reasoning effort
    pub fn recommended_timeout_secs(&self) -> u64 {
        match self.reasoning_effort.as_str() {
            "low" => 30,
            "medium" => 60,
            "high" => 120,
            _ => 60, // Default fallback
        }
    }

    /// Convert to JSON for API requests
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "verbosity": self.verbosity,
            "reasoning_effort": self.reasoning_effort,
            "max_output_tokens": self.max_output_tokens
        })
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self::from_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let mut config = ClientConfig::default();
        config.api_key = "test-key".to_string();
        
        assert!(config.validate().is_ok());
        
        // Test empty API key
        config.api_key = "".to_string();
        assert!(config.validate().is_err());
        
        // Test invalid verbosity
        config.api_key = "test-key".to_string();
        config.verbosity = "invalid".to_string();
        assert!(config.validate().is_err());
        
        config.verbosity = "medium".to_string();
        assert!(config.validate().is_ok());
        
        // Test invalid max tokens
        config.max_output_tokens = 0;
        assert!(config.validate().is_err());
        
        config.max_output_tokens = 300000;
        assert!(config.validate().is_err());
        
        config.max_output_tokens = 128000;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_model_config() {
        let config = ModelConfig::default();
        
        assert!(config.supports_streaming());
        // THE FIX: The default reasoning_effort is "high", which corresponds to 120 seconds.
        assert_eq!(config.recommended_timeout_secs(), 120); 
        
        let json = config.to_json();
        assert!(json["model"].is_string());
        assert!(json["verbosity"].is_string());
    }

    #[test]
    fn test_headers() {
        let config = ClientConfig {
            api_key: "test-key".to_string(),
            ..Default::default()
        };
        
        let headers = config.default_headers();
        assert_eq!(headers.len(), 3);
        assert!(headers.iter().any(|(k, v)| k == "authorization" && v.contains("test-key")));
        assert!(headers.iter().any(|(k, v)| k == "content-type" && v == "application/json"));
    }

    #[test]
    fn test_reasoning_effort_timeout() {
        let mut config = ModelConfig::default();
        
        config.reasoning_effort = "low".to_string();
        assert_eq!(config.recommended_timeout_secs(), 30);
        
        config.reasoning_effort = "medium".to_string();
        assert_eq!(config.recommended_timeout_secs(), 60);
        
        config.reasoning_effort = "high".to_string();
        assert_eq!(config.recommended_timeout_secs(), 120);
    }
}
