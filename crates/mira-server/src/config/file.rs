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
    /// Provider for expert tools (consult_architect, consult_code_reviewer, etc.)
    pub expert_provider: Option<String>,
    /// Provider for background intelligence (summaries, briefings, capabilities, code health)
    pub background_provider: Option<String>,
}

impl MiraConfig {
    /// Load config from ~/.mira/config.toml
    pub fn load() -> Self {
        let path = Self::config_path();

        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                match toml::from_str(&contents) {
                    Ok(config) => {
                        debug!(path = %path.display(), "Loaded config from file");
                        config
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "Failed to parse config file");
                        Self::default()
                    }
                }
            }
            Err(_) => {
                debug!(path = %path.display(), "Config file not found, using defaults");
                Self::default()
            }
        }
    }

    /// Get the config file path
    fn config_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".mira")
            .join("config.toml")
    }

    /// Get the expert tools LLM provider from config
    pub fn expert_provider(&self) -> Option<Provider> {
        self.llm
            .expert_provider
            .as_deref()
            .and_then(Provider::from_str)
    }

    /// Get the background intelligence LLM provider from config
    pub fn background_provider(&self) -> Option<Provider> {
        self.llm
            .background_provider
            .as_deref()
            .and_then(Provider::from_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let toml = r#"
[llm]
expert_provider = "glm"
background_provider = "deepseek"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.expert_provider(), Some(Provider::Glm));
        assert_eq!(config.background_provider(), Some(Provider::DeepSeek));
    }

    #[test]
    fn test_parse_empty_config() {
        let config: MiraConfig = toml::from_str("").unwrap();
        assert_eq!(config.expert_provider(), None);
        assert_eq!(config.background_provider(), None);
    }

    #[test]
    fn test_default_config() {
        let config = MiraConfig::default();
        assert_eq!(config.expert_provider(), None);
        assert_eq!(config.background_provider(), None);
    }
}
