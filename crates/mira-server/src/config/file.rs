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
    #[serde(default)]
    pub retention: RetentionConfig,
}

/// Data retention configuration section
#[derive(Debug, Deserialize, Clone)]
pub struct RetentionConfig {
    /// Master switch -- enabled by default for automatic data hygiene
    #[serde(default = "RetentionConfig::default_enabled")]
    pub enabled: bool,
    /// Days to keep tool_history, chat_messages, chat_summaries, etc.
    #[serde(default = "RetentionConfig::default_tool_history_days")]
    pub tool_history_days: u32,
    /// Days to keep chat data
    #[serde(default = "RetentionConfig::default_chat_days")]
    pub chat_days: u32,
    /// Days to keep session data
    #[serde(default = "RetentionConfig::default_sessions_days")]
    pub sessions_days: u32,
    /// Days to keep analytics (llm_usage, embeddings_usage)
    #[serde(default = "RetentionConfig::default_analytics_days")]
    pub analytics_days: u32,
    /// Days to keep behavior patterns
    #[serde(default = "RetentionConfig::default_behavior_days")]
    pub behavior_days: u32,
    /// Days to keep system observations
    #[serde(default = "RetentionConfig::default_observations_days")]
    pub observations_days: u32,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tool_history_days: 30,
            chat_days: 30,
            sessions_days: 90,
            analytics_days: 180,
            behavior_days: 365,
            observations_days: 90,
        }
    }
}

impl RetentionConfig {
    fn default_enabled() -> bool {
        true
    }
    fn default_tool_history_days() -> u32 {
        30
    }
    fn default_chat_days() -> u32 {
        30
    }
    fn default_sessions_days() -> u32 {
        90
    }
    fn default_analytics_days() -> u32 {
        180
    }
    fn default_behavior_days() -> u32 {
        365
    }
    fn default_observations_days() -> u32 {
        90
    }

    /// Check if retention is enabled (config field OR env var override)
    pub fn is_enabled(&self) -> bool {
        if let Some(env_val) = crate::config::env::parse_bool_env("MIRA_RETENTION_ENABLED") {
            return env_val;
        }
        self.enabled
    }
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
            .unwrap_or_else(|| {
                warn!("HOME directory not set — using current directory for Mira config. This may cause config files to be created in your project directory. Consider setting $HOME.");
                PathBuf::from(".")
            })
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
default_provider = "ollama"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.default_provider(), Some(Provider::Ollama));
    }

    #[test]
    fn test_parse_both_providers() {
        let toml = r#"
[llm]
background_provider = "ollama"
default_provider = "deepseek"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.background_provider(), Some(Provider::Ollama));
        assert_eq!(config.default_provider(), Some(Provider::DeepSeek));
    }

    #[test]
    fn test_default_config() {
        let config = MiraConfig::default();
        assert_eq!(config.background_provider(), None);
        assert_eq!(config.default_provider(), None);
    }

    #[test]
    fn test_corrupt_toml_falls_back_to_default() {
        // Malformed TOML should parse-fail, not panic
        let bad_toml = r#"
[llm
background_provider = broken
"#;
        let result: Result<MiraConfig, _> = toml::from_str(bad_toml);
        assert!(result.is_err(), "Corrupt TOML should fail to parse");

        // In production, load() would return Self::default()
        let config = result.unwrap_or_default();
        assert_eq!(config.background_provider(), None);
        assert_eq!(config.default_provider(), None);
    }

    #[test]
    fn test_unknown_keys_ignored() {
        // Serde ignores unknown fields by default (no deny_unknown_fields)
        let toml = r#"
[llm]
background_provider = "deepseek"
unknown_key = "should be ignored"
typo_provider = "zhipu"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.background_provider(), Some(Provider::DeepSeek));
    }

    #[test]
    fn test_unknown_sections_ignored() {
        let toml = r#"
[llm]
background_provider = "ollama"

[database]
path = "/tmp/test.db"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.background_provider(), Some(Provider::Ollama));
    }

    #[test]
    fn test_invalid_provider_name_returns_none() {
        let toml = r#"
[llm]
background_provider = "gpt4"
default_provider = "claude"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        // Invalid provider names parse as strings but from_str returns None
        assert_eq!(config.background_provider(), None);
        assert_eq!(config.default_provider(), None);
    }

    #[test]
    fn test_removed_provider_alias_returns_none() {
        // "glm" was an alias for the removed Zhipu provider — now invalid
        let toml = r#"
[llm]
background_provider = "glm"
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.background_provider(), None);
    }

    #[test]
    fn test_wrong_type_for_provider_fails_parse() {
        // Provider as integer instead of string
        let toml = r#"
[llm]
background_provider = 123
"#;
        let result: Result<MiraConfig, _> = toml::from_str(toml);
        assert!(result.is_err(), "Wrong type should fail to parse");
    }

    // ═══════════════════════════════════════
    // RetentionConfig tests
    // ═══════════════════════════════════════

    #[test]
    fn test_retention_defaults() {
        let config: MiraConfig = toml::from_str("").unwrap();
        assert!(config.retention.enabled);
        assert_eq!(config.retention.tool_history_days, 30);
        assert_eq!(config.retention.chat_days, 30);
        assert_eq!(config.retention.sessions_days, 90);
        assert_eq!(config.retention.analytics_days, 180);
        assert_eq!(config.retention.behavior_days, 365);
        assert_eq!(config.retention.observations_days, 90);
    }

    #[test]
    fn test_retention_from_toml() {
        let toml = r#"
[retention]
enabled = true
tool_history_days = 14
chat_days = 7
sessions_days = 60
analytics_days = 90
behavior_days = 180
observations_days = 45
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert!(config.retention.enabled);
        assert_eq!(config.retention.tool_history_days, 14);
        assert_eq!(config.retention.chat_days, 7);
        assert_eq!(config.retention.sessions_days, 60);
        assert_eq!(config.retention.analytics_days, 90);
        assert_eq!(config.retention.behavior_days, 180);
        assert_eq!(config.retention.observations_days, 45);
    }

    #[test]
    fn test_retention_partial_config_uses_defaults() {
        let toml = r#"
[retention]
enabled = true
tool_history_days = 14
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert!(config.retention.enabled);
        assert_eq!(config.retention.tool_history_days, 14);
        // All others should be defaults
        assert_eq!(config.retention.chat_days, 30);
        assert_eq!(config.retention.sessions_days, 90);
        assert_eq!(config.retention.analytics_days, 180);
        assert_eq!(config.retention.behavior_days, 365);
        assert_eq!(config.retention.observations_days, 90);
    }

    #[test]
    fn test_retention_unknown_keys_ignored() {
        let toml = r#"
[retention]
enabled = true
unknown_retention_key = 42
"#;
        let config: MiraConfig = toml::from_str(toml).unwrap();
        assert!(config.retention.enabled);
    }

    #[test]
    fn test_retention_is_enabled_by_default() {
        let config = RetentionConfig::default();
        assert!(config.is_enabled());
    }

    #[test]
    fn test_retention_is_enabled_config_true() {
        let config = RetentionConfig {
            enabled: true,
            ..Default::default()
        };
        assert!(config.is_enabled());
    }

    #[test]
    fn test_retention_env_override() {
        // SAFETY: test-only, single-threaded test runner for this module
        unsafe {
            std::env::set_var("MIRA_RETENTION_ENABLED", "true");
        }
        let config = RetentionConfig::default();
        assert!(config.is_enabled());
        unsafe {
            std::env::remove_var("MIRA_RETENTION_ENABLED");
        }
    }

    #[test]
    fn test_retention_env_override_numeric() {
        // SAFETY: test-only, single-threaded test runner for this module
        unsafe {
            std::env::set_var("MIRA_RETENTION_ENABLED", "1");
        }
        let config = RetentionConfig::default();
        assert!(config.is_enabled());
        unsafe {
            std::env::remove_var("MIRA_RETENTION_ENABLED");
        }
    }
}
