// crates/mira-server/src/config/file.rs
// File-based configuration from ~/.mira/config.toml

use crate::llm::Provider;
use serde::Deserialize;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Top-level config structure
#[derive(Debug, Deserialize, Default)]
pub struct MiraConfig {
    #[serde(default)]
    pub llm: LlmConfig,
}

/// LLM configuration section
#[derive(Debug, Deserialize, Default)]
pub struct LlmConfig {
    /// Provider for background intelligence (summaries, briefings, capabilities, code health)
    pub background_provider: Option<String>,
    /// Default provider for all other LLM tasks (overrides DEFAULT_LLM_PROVIDER env var)
    pub default_provider: Option<String>,
}

impl MiraConfig {
    /// Load config from ~/.mira/config.toml
    pub fn load() -> Self {
        let path = Self::config_path();

        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => {
                    debug!(path = %path.display(), "Loaded config from file");
                    config
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to parse config file");
                    Self::default()
                }
            },
            Err(_) => {
                debug!(path = %path.display(), "Config file not found, using defaults");
                Self::default()
            }
        }
    }

    /// Get the background intelligence LLM provider from config
    pub fn background_provider(&self) -> Option<Provider> {
        self.llm
            .background_provider
            .as_deref()
            .and_then(Provider::from_str)
    }

    /// Get the default LLM provider from config
    pub fn default_provider(&self) -> Option<Provider> {
        self.llm
            .default_provider
            .as_deref()
            .and_then(Provider::from_str)
    }

    /// Get the config file path (public for CLI config commands)
    pub fn config_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".mira")
            .join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let toml = r#"
[llm]
background_provider = "deepseek"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.background_provider(), Some(Provider::DeepSeek));
    }

    #[test]
    fn test_parse_empty_config() {
        let config: MiraConfig = toml::from_str("").unwrap();
        assert_eq!(config.background_provider(), None);
    }

    #[test]
    fn test_parse_default_provider() {
        let toml = r#"
[llm]
default_provider = "zhipu"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.default_provider(), Some(Provider::Zhipu));
    }

    #[test]
    fn test_parse_both_providers() {
        let toml = r#"
[llm]
background_provider = "zhipu"
default_provider = "deepseek"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.background_provider(), Some(Provider::Zhipu));
        assert_eq!(config.default_provider(), Some(Provider::DeepSeek));
    }

    #[test]
    fn test_default_config() {
        let config = MiraConfig::default();
        assert_eq!(config.background_provider(), None);
        assert_eq!(config.default_provider(), None);
    }
}
