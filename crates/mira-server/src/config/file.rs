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
    /// Default provider for expert consultations
    pub default_provider: Option<String>,
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

    /// Get the default LLM provider from config
    pub fn default_provider(&self) -> Option<Provider> {
        self.llm
            .default_provider
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
default_provider = "glm"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.default_provider(), Some(Provider::Glm));
    }

    #[test]
    fn test_parse_empty_config() {
        let config: MiraConfig = toml::from_str("").unwrap();
        assert_eq!(config.default_provider(), None);
    }

    #[test]
    fn test_default_config() {
        let config = MiraConfig::default();
        assert_eq!(config.default_provider(), None);
    }
}
