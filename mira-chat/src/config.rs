//! Configuration file support for mira-chat
//!
//! Loads config from ~/.mira/config.toml

use serde::Deserialize;
use std::path::PathBuf;

/// Configuration for mira-chat
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// OpenAI API key
    pub openai_api_key: Option<String>,

    /// Gemini API key for embeddings
    pub gemini_api_key: Option<String>,

    /// Database URL
    pub database_url: Option<String>,

    /// Qdrant URL
    pub qdrant_url: Option<String>,

    /// Default reasoning effort
    pub reasoning_effort: Option<String>,

    /// Default project path
    pub project: Option<String>,
}

impl Config {
    /// Load config from ~/.mira/config.toml
    pub fn load() -> Self {
        let path = config_path();

        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
                    Self::default()
                }
            },
            Err(e) => {
                eprintln!("Warning: Failed to read {}: {}", path.display(), e);
                Self::default()
            }
        }
    }

    /// Get a value with fallback to environment variable
    pub fn get_or_env(&self, field: Option<&String>, env_var: &str) -> Option<String> {
        field
            .cloned()
            .or_else(|| std::env::var(env_var).ok())
    }
}

/// Get the config file path
pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".mira")
        .join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.openai_api_key.is_none());
        assert!(config.gemini_api_key.is_none());
    }

    #[test]
    fn test_config_path() {
        let path = config_path();
        assert!(path.to_string_lossy().contains(".mira"));
        assert!(path.to_string_lossy().ends_with("config.toml"));
    }
}
