// src/llm/client/config.rs
// Phase 1: Extract Configuration Management from client.rs
// Centralizes all LLM client configuration and environment variable handling

use anyhow::Result;
use tracing::info;

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
    /// Create configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY must be set"))?;
        
        let model = std::env::var("MIRA_MODEL")
            .unwrap_or_else(|_| "gpt-5".to_string());
        
        let verbosity = std::env::var("MIRA_VERBOSITY")
            .unwrap_or_else(|_| "medium".to_string());
        
        let reasoning_effort = std::env::var("MIRA_REASONING_EFFORT")
            .unwrap_or_else(|_| "medium".to_string());
        
        let max_output_tokens = std::env::var("MIRA_MAX_OUTPUT_TOKENS")
            .unwrap_or_else(|_| "128000".to_string())
            .parse()
            .unwrap_or(128000);
        
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_string());

        info!(
            "🚀 Initialized LLM client config (model={}, verbosity={}, reasoning={}, max_tokens={})",
            model, verbosity, reasoning_effort, max_output_tokens
        );

        Ok(Self {
            api_key,
            base_url,
            model,
            verbosity,
            reasoning_effort,
            max_output_tokens,
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

        if self.model.is_empty() {
            return Err(anyhow::anyhow!("Model cannot be empty"));
        }

        // Validate verbosity levels
        match self.verbosity.as_str() {
            "low" | "medium" | "high" => {},
            _ => return Err(anyhow::anyhow!("Invalid verbosity level: {}", self.verbosity)),
        }

        // Validate reasoning effort levels
        match self.reasoning_effort.as_str() {
            "low" | "medium" | "high" => {},
            _ => return Err(anyhow::anyhow!("Invalid reasoning effort level: {}", self.reasoning_effort)),
        }

        // Validate max output tokens
        if self.max_output_tokens == 0 {
            return Err(anyhow::anyhow!("Max output tokens must be greater than 0"));
        }

        if self.max_output_tokens > 200000 {
            return Err(anyhow::anyhow!("Max output tokens too large: {}", self.max_output_tokens));
        }

        Ok(())
    }

    /// Get default headers for requests
    pub fn default_headers(&self) -> Vec<(String, String)> {
        vec![
            ("authorization".to_string(), format!("Bearer {}", self.api_key)),
            ("content-type".to_string(), "application/json".to_string()),
            ("user-agent".to_string(), "mira-backend/0.4.1".to_string()),
        ]
    }

    /// Get model-specific configuration
    pub fn model_config(&self) -> ModelConfig {
        ModelConfig {
            model: self.model.clone(),
            verbosity: self.verbosity.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            max_output_tokens: self.max_output_tokens,
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
    /// Convert to JSON value for API requests
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "verbosity": self.verbosity,
            "reasoning_effort": self.reasoning_effort,
            "max_output_tokens": self.max_output_tokens
        })
    }

    /// Check if model supports streaming
    pub fn supports_streaming(&self) -> bool {
        // Most modern models support streaming
        true
    }

    /// Check if model supports structured output
    pub fn supports_structured_output(&self) -> bool {
        // GPT-5 and modern models support structured output
        self.model.starts_with("gpt-5") || 
        self.model.starts_with("gpt-4") ||
        self.model.contains("turbo")
    }

    /// Get recommended timeout for this model
    pub fn recommended_timeout_secs(&self) -> u64 {
        match self.reasoning_effort.as_str() {
            "high" => 120, // High reasoning effort needs more time
            "medium" => 60,
            "low" => 30,
            _ => 60,
        }
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com".to_string(),
            model: "gpt-5".to_string(),
            verbosity: "medium".to_string(),
            reasoning_effort: "medium".to_string(),
            max_output_tokens: 128000,
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model: "gpt-5".to_string(),
            verbosity: "medium".to_string(),
            reasoning_effort: "medium".to_string(),
            max_output_tokens: 128000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let mut config = ClientConfig::default();
        
        // Should fail with empty API key
        assert!(config.validate().is_err());
        
        // Set valid API key
        config.api_key = "test-key".to_string();
        assert!(config.validate().is_ok());
        
        // Test invalid verbosity
        config.verbosity = "invalid".to_string();
        assert!(config.validate().is_err());
        
        // Reset to valid
        config.verbosity = "medium".to_string();
        assert!(config.validate().is_ok());
        
        // Test invalid reasoning effort
        config.reasoning_effort = "invalid".to_string();
        assert!(config.validate().is_err());
        
        // Reset to valid
        config.reasoning_effort = "high".to_string();
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
        assert!(config.supports_structured_output());
        assert_eq!(config.recommended_timeout_secs(), 60);
        
        let json = config.to_json();
        assert_eq!(json["model"], "gpt-5");
        assert_eq!(json["verbosity"], "medium");
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
