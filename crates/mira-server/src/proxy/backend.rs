// crates/mira-server/src/proxy/backend.rs
// Backend configuration and client management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// API type for backend routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ApiType {
    /// Anthropic Messages API (/v1/messages)
    #[default]
    Anthropic,
    /// OpenAI-compatible API format (/v1/chat/completions)
    /// Note: Refers to the API protocol format, not the OpenAI service
    /// Used by DeepSeek and other providers following this standard
    Openai,
}

/// Pricing configuration for cost estimation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PricingConfig {
    /// Cost per million input tokens (USD)
    #[serde(default)]
    pub input_per_million: f64,
    /// Cost per million output tokens (USD)
    #[serde(default)]
    pub output_per_million: f64,
    /// Cost per million cache creation tokens (USD, optional)
    #[serde(default)]
    pub cache_creation_per_million: f64,
    /// Cost per million cache read tokens (USD, optional)
    #[serde(default)]
    pub cache_read_per_million: f64,
}

impl PricingConfig {
    /// Calculate cost for a request
    pub fn calculate_cost(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cache_creation_tokens: u64,
        cache_read_tokens: u64,
    ) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_per_million;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_per_million;
        let cache_creation_cost = (cache_creation_tokens as f64 / 1_000_000.0) * self.cache_creation_per_million;
        let cache_read_cost = (cache_read_tokens as f64 / 1_000_000.0) * self.cache_read_per_million;
        input_cost + output_cost + cache_creation_cost + cache_read_cost
    }
}

/// Configuration for a single LLM backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Display name for this backend
    pub name: String,
    /// Base URL for the API (e.g., "https://api.anthropic.com")
    pub base_url: String,
    /// API key (inline, not recommended for production)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Environment variable containing the API key (preferred)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Whether this backend is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// API type (anthropic or openai-compatible format) - affects endpoint and request format
    #[serde(default)]
    pub api_type: ApiType,
    /// Optional model mapping (proxy model name -> backend model name)
    #[serde(default)]
    pub model_map: HashMap<String, String>,
    /// Claude Code environment variable overrides for this backend
    /// (e.g., ANTHROPIC_MODEL, API_TIMEOUT_MS, etc.)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    /// Pricing configuration for cost estimation
    #[serde(default)]
    pub pricing: PricingConfig,
}

fn default_true() -> bool {
    true
}

impl BackendConfig {
    /// Get the API key, checking env var first then inline
    pub fn get_api_key(&self) -> Option<String> {
        if let Some(env_var) = &self.api_key_env {
            if let Ok(key) = std::env::var(env_var) {
                if !key.is_empty() {
                    return Some(key);
                }
            }
        }
        self.api_key.clone()
    }

    /// Check if this backend is usable (enabled + has API key)
    pub fn is_usable(&self) -> bool {
        self.enabled && self.get_api_key().is_some()
    }
}

/// A configured and ready-to-use backend
#[derive(Debug, Clone)]
pub struct Backend {
    pub config: BackendConfig,
    pub client: reqwest::Client,
}

impl Backend {
    pub fn new(config: BackendConfig) -> Self {
        let client = reqwest::Client::new();
        Self { config, client }
    }

    /// Create a new backend with a shared HTTP client
    pub fn with_http_client(config: BackendConfig, client: reqwest::Client) -> Self {
        Self { config, client }
    }
}

/// Top-level proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Port to listen on (default: 8100)
    #[serde(default = "default_port")]
    pub port: u16,
    /// Host to bind to (default: 127.0.0.1)
    #[serde(default = "default_host")]
    pub host: String,
    /// Default backend to use when none specified
    #[serde(default)]
    pub default_backend: Option<String>,
    /// Configured backends
    #[serde(default)]
    pub backends: HashMap<String, BackendConfig>,
}

fn default_port() -> u16 {
    8100
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            default_backend: None,
            backends: HashMap::new(),
        }
    }
}

impl ProxyConfig {
    /// Load config from the default location (~/.config/mira/proxy.toml)
    pub fn load() -> anyhow::Result<Self> {
        let config_path = Self::default_config_path()?;
        if config_path.exists() {
            Self::load_from(&config_path)
        } else {
            Ok(Self::default())
        }
    }

    /// Load config from a specific path
    pub fn load_from(path: &PathBuf) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: ProxyConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Get the default config path
    pub fn default_config_path() -> anyhow::Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
        Ok(config_dir.join("mira").join("proxy.toml"))
    }

    /// Get a backend by name
    pub fn get_backend(&self, name: &str) -> Option<&BackendConfig> {
        self.backends.get(name)
    }

    /// Get the default backend config
    pub fn get_default_backend(&self) -> Option<&BackendConfig> {
        self.default_backend
            .as_ref()
            .and_then(|name| self.backends.get(name))
    }

    /// List all usable backends
    pub fn usable_backends(&self) -> Vec<(&String, &BackendConfig)> {
        self.backends
            .iter()
            .filter(|(_, config)| config.is_usable())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ProxyConfig::default();
        assert_eq!(config.port, 8100);
        assert_eq!(config.host, "127.0.0.1");
        assert!(config.backends.is_empty());
    }

    #[test]
    fn test_backend_api_key_from_env() {
        let config = BackendConfig {
            name: "test".to_string(),
            base_url: "https://api.example.com".to_string(),
            api_key: Some("inline-key".to_string()),
            api_key_env: Some("TEST_API_KEY".to_string()),
            enabled: true,
            api_type: ApiType::Anthropic,
            model_map: HashMap::new(),
            env: HashMap::new(),
            pricing: PricingConfig::default(),
        };

        // Env var takes precedence when set
        // SAFETY: Test runs single-threaded, no concurrent env access
        unsafe { std::env::set_var("TEST_API_KEY", "env-key") };
        assert_eq!(config.get_api_key(), Some("env-key".to_string()));
        unsafe { std::env::remove_var("TEST_API_KEY") };

        // Falls back to inline when env not set
        assert_eq!(config.get_api_key(), Some("inline-key".to_string()));
    }

    #[test]
    fn test_parse_toml() {
        let toml_str = r#"
port = 8200
host = "0.0.0.0"
default_backend = "anthropic"

[backends.anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com"
api_key_env = "ANTHROPIC_API_KEY"

[backends.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
api_key_env = "DEEPSEEK_API_KEY"
enabled = false
"#;

        let config: ProxyConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.port, 8200);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.default_backend, Some("anthropic".to_string()));
        assert_eq!(config.backends.len(), 2);
        assert!(config.backends.get("anthropic").unwrap().enabled);
        assert!(!config.backends.get("deepseek").unwrap().enabled);
    }
}
