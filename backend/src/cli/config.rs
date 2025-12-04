// backend/src/cli/config.rs
// CLI configuration management

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// CLI configuration loaded from ~/.mira/config.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// Backend WebSocket URL
    #[serde(default = "default_backend_url")]
    pub backend_url: String,

    /// Default output format
    #[serde(default)]
    pub default_output_format: String,

    /// Whether to show thinking by default
    #[serde(default)]
    pub show_thinking: bool,

    /// Default verbose mode
    #[serde(default)]
    pub verbose: bool,

    /// Theme/color scheme
    #[serde(default = "default_theme")]
    pub theme: String,

    /// History file path (relative to ~/.mira/)
    #[serde(default = "default_history_file")]
    pub history_file: String,

    /// Maximum history entries
    #[serde(default = "default_max_history")]
    pub max_history: usize,
}

fn default_backend_url() -> String {
    "ws://localhost:3001/ws".to_string()
}

fn default_theme() -> String {
    "default".to_string()
}

fn default_history_file() -> String {
    "history".to_string()
}

fn default_max_history() -> usize {
    1000
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            backend_url: default_backend_url(),
            default_output_format: "text".to_string(),
            show_thinking: false,
            verbose: false,
            theme: default_theme(),
            history_file: default_history_file(),
            max_history: default_max_history(),
        }
    }
}

impl CliConfig {
    /// Load configuration from ~/.mira/config.json
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config file: {:?}", config_path))?;
            let config: Self = serde_json::from_str(&content)
                .with_context(|| "Failed to parse config file")?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Save configuration to ~/.mira/config.json
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;

        // Ensure directory exists
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
        }

        let content = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize config")?;
        std::fs::write(&config_path, content)
            .with_context(|| format!("Failed to write config file: {:?}", config_path))?;

        Ok(())
    }

    /// Get the config file path
    pub fn config_path() -> Result<PathBuf> {
        let mira_dir = Self::mira_dir()?;
        Ok(mira_dir.join("config.json"))
    }

    /// Get the ~/.mira directory path
    pub fn mira_dir() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .context("Could not determine home directory")?;
        Ok(home.join(".mira"))
    }

    /// Get the CLI database path (~/.mira/cli.db)
    pub fn cli_db_path() -> Result<PathBuf> {
        let mira_dir = Self::mira_dir()?;
        Ok(mira_dir.join("cli.db"))
    }

    /// Get the history file path
    pub fn history_path(&self) -> Result<PathBuf> {
        let mira_dir = Self::mira_dir()?;
        Ok(mira_dir.join(&self.history_file))
    }

    /// Get the commands directory (~/.mira/commands/)
    pub fn commands_dir() -> Result<PathBuf> {
        let mira_dir = Self::mira_dir()?;
        Ok(mira_dir.join("commands"))
    }

    /// Get the agents directory (~/.mira/agents/)
    pub fn agents_dir() -> Result<PathBuf> {
        let mira_dir = Self::mira_dir()?;
        Ok(mira_dir.join("agents"))
    }

    /// Ensure the ~/.mira directory structure exists
    pub fn ensure_dirs() -> Result<()> {
        let mira_dir = Self::mira_dir()?;
        std::fs::create_dir_all(&mira_dir)
            .with_context(|| format!("Failed to create ~/.mira directory: {:?}", mira_dir))?;

        let commands_dir = Self::commands_dir()?;
        std::fs::create_dir_all(&commands_dir)
            .with_context(|| format!("Failed to create commands directory: {:?}", commands_dir))?;

        let agents_dir = Self::agents_dir()?;
        std::fs::create_dir_all(&agents_dir)
            .with_context(|| format!("Failed to create agents directory: {:?}", agents_dir))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CliConfig::default();
        assert_eq!(config.backend_url, "ws://localhost:3001/ws");
        assert_eq!(config.default_output_format, "text");
        assert!(!config.show_thinking);
        assert!(!config.verbose);
    }

    #[test]
    fn test_config_serialization() {
        let config = CliConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: CliConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.backend_url, parsed.backend_url);
    }
}
