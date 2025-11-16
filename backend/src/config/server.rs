// src/config/server.rs
// Server, database, and infrastructure configuration

use serde::{Deserialize, Serialize};

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        Self {
            host: super::helpers::require_env("MIRA_HOST"),
            port: super::helpers::require_env_parsed("MIRA_PORT"),
        }
    }

    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub timeout: u64,
}

impl DatabaseConfig {
    pub fn from_env() -> Self {
        Self {
            url: super::helpers::require_env("DATABASE_URL"),
            max_connections: super::helpers::require_env_parsed("MIRA_SQLITE_MAX_CONNECTIONS"),
            timeout: super::helpers::require_env_parsed("DATABASE_TIMEOUT"),
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub trace_sql: bool,
    pub debug_logging: bool,
}

impl LoggingConfig {
    pub fn from_env() -> Self {
        Self {
            level: super::helpers::require_env("MIRA_LOG_LEVEL"),
            trace_sql: super::helpers::require_env_parsed("MIRA_TRACE_SQL"),
            debug_logging: super::helpers::require_env_parsed("MIRA_DEBUG_LOGGING"),
        }
    }
}

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub session_id: String,
    pub default_persona: String,
}

impl SessionConfig {
    pub fn from_env() -> Self {
        Self {
            session_id: super::helpers::require_env("MIRA_SESSION_ID"),
            default_persona: super::helpers::require_env("MIRA_DEFAULT_PERSONA"),
        }
    }
}
