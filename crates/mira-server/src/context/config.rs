// crates/mira-server/src/context/config.rs
// Configuration for proactive context injection

use crate::db::pool::DatabasePool;
use crate::db::{get_server_state_sync, set_server_state_sync};
use anyhow::Result;
use std::sync::Arc;

/// Configuration key prefix for injection settings
const CONFIG_PREFIX: &str = "injection_";

/// Configuration for context injection behavior
#[derive(Debug, Clone)]
pub struct InjectionConfig {
    /// Whether injection is enabled
    pub enabled: bool,
    /// Maximum characters for injected context
    pub max_chars: usize,
    /// Minimum message length to trigger injection
    pub min_message_len: usize,
    /// Maximum message length to trigger injection (skip code pastes)
    pub max_message_len: usize,
    /// Sample rate (0.0 to 1.0) - fraction of eligible messages to inject
    pub sample_rate: f64,
    /// Enable semantic search injection
    pub enable_semantic: bool,
    /// Enable file-aware injection
    pub enable_file_aware: bool,
    /// Enable task-aware injection
    pub enable_task_aware: bool,
}

impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_chars: 1500,
            min_message_len: 30,
            max_message_len: 500,
            sample_rate: 0.5, // 50% of eligible messages
            enable_semantic: true,
            enable_file_aware: true,
            enable_task_aware: true,
        }
    }
}

impl InjectionConfig {
    /// Load configuration from database
    pub async fn load(pool: &Arc<DatabasePool>) -> Result<Self> {
        let pool = pool.clone();
        pool.interact(move |conn| {
            let mut config = Self::default();

            if let Ok(Some(v)) = get_server_state_sync(conn, &format!("{}enabled", CONFIG_PREFIX)) {
                config.enabled = v == "true";
            }
            if let Ok(Some(v)) = get_server_state_sync(conn, &format!("{}max_chars", CONFIG_PREFIX)) {
                if let Ok(n) = v.parse() {
                    config.max_chars = n;
                }
            }
            if let Ok(Some(v)) = get_server_state_sync(conn, &format!("{}min_message_len", CONFIG_PREFIX)) {
                if let Ok(n) = v.parse() {
                    config.min_message_len = n;
                }
            }
            if let Ok(Some(v)) = get_server_state_sync(conn, &format!("{}max_message_len", CONFIG_PREFIX)) {
                if let Ok(n) = v.parse() {
                    config.max_message_len = n;
                }
            }
            if let Ok(Some(v)) = get_server_state_sync(conn, &format!("{}sample_rate", CONFIG_PREFIX)) {
                if let Ok(n) = v.parse() {
                    config.sample_rate = n;
                }
            }
            if let Ok(Some(v)) = get_server_state_sync(conn, &format!("{}enable_semantic", CONFIG_PREFIX)) {
                config.enable_semantic = v == "true";
            }
            if let Ok(Some(v)) = get_server_state_sync(conn, &format!("{}enable_file_aware", CONFIG_PREFIX)) {
                config.enable_file_aware = v == "true";
            }
            if let Ok(Some(v)) = get_server_state_sync(conn, &format!("{}enable_task_aware", CONFIG_PREFIX)) {
                config.enable_task_aware = v == "true";
            }

            Ok(config)
        }).await
    }

    /// Save configuration to database
    pub async fn save(&self, pool: &Arc<DatabasePool>) -> Result<()> {
        let config = self.clone();
        pool.interact(move |conn| {
            set_server_state_sync(conn, &format!("{}enabled", CONFIG_PREFIX), &config.enabled.to_string())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            set_server_state_sync(conn, &format!("{}max_chars", CONFIG_PREFIX), &config.max_chars.to_string())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            set_server_state_sync(conn, &format!("{}min_message_len", CONFIG_PREFIX), &config.min_message_len.to_string())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            set_server_state_sync(conn, &format!("{}max_message_len", CONFIG_PREFIX), &config.max_message_len.to_string())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            set_server_state_sync(conn, &format!("{}sample_rate", CONFIG_PREFIX), &config.sample_rate.to_string())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            set_server_state_sync(conn, &format!("{}enable_semantic", CONFIG_PREFIX), &config.enable_semantic.to_string())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            set_server_state_sync(conn, &format!("{}enable_file_aware", CONFIG_PREFIX), &config.enable_file_aware.to_string())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            set_server_state_sync(conn, &format!("{}enable_task_aware", CONFIG_PREFIX), &config.enable_task_aware.to_string())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok(())
        }).await
    }

    /// Create a builder for fluent configuration
    pub fn builder() -> InjectionConfigBuilder {
        InjectionConfigBuilder::default()
    }

    /// Format as human-readable summary
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if !self.enabled {
            return "Context injection: disabled".to_string();
        }

        parts.push(format!("max_chars={}", self.max_chars));
        parts.push(format!("sample_rate={:.0}%", self.sample_rate * 100.0));

        let mut sources = Vec::new();
        if self.enable_semantic {
            sources.push("semantic");
        }
        if self.enable_file_aware {
            sources.push("files");
        }
        if self.enable_task_aware {
            sources.push("tasks");
        }
        parts.push(format!("sources=[{}]", sources.join(",")));

        format!("Context injection: {}", parts.join(", "))
    }
}

/// Builder for InjectionConfig
#[derive(Default)]
pub struct InjectionConfigBuilder {
    config: InjectionConfig,
}

impl InjectionConfigBuilder {
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.config.enabled = enabled;
        self
    }

    pub fn max_chars(mut self, max_chars: usize) -> Self {
        self.config.max_chars = max_chars;
        self
    }

    pub fn sample_rate(mut self, rate: f64) -> Self {
        self.config.sample_rate = rate.clamp(0.0, 1.0);
        self
    }

    pub fn enable_semantic(mut self, enable: bool) -> Self {
        self.config.enable_semantic = enable;
        self
    }

    pub fn enable_file_aware(mut self, enable: bool) -> Self {
        self.config.enable_file_aware = enable;
        self
    }

    pub fn enable_task_aware(mut self, enable: bool) -> Self {
        self.config.enable_task_aware = enable;
        self
    }

    pub fn build(self) -> InjectionConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = InjectionConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_chars, 1500);
        assert_eq!(config.sample_rate, 0.5);
    }

    #[test]
    fn test_builder() {
        let config = InjectionConfig::builder()
            .enabled(false)
            .max_chars(2000)
            .sample_rate(0.75)
            .enable_task_aware(false)
            .build();

        assert!(!config.enabled);
        assert_eq!(config.max_chars, 2000);
        assert_eq!(config.sample_rate, 0.75);
        assert!(!config.enable_task_aware);
    }

    #[test]
    fn test_summary() {
        let config = InjectionConfig::default();
        let summary = config.summary();
        assert!(summary.contains("1500"));
        assert!(summary.contains("50%"));
    }

    #[test]
    fn test_disabled_summary() {
        let config = InjectionConfig::builder().enabled(false).build();
        assert_eq!(config.summary(), "Context injection: disabled");
    }
}
