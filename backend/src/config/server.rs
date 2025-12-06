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
}

impl LoggingConfig {
    pub fn from_env() -> Self {
        Self {
            level: super::helpers::require_env("MIRA_LOG_LEVEL"),
        }
    }
}

/// Rate limiting configuration with tiered limits based on context size
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub enabled: bool,
    /// Standard rate limit for requests with <200k context
    pub requests_per_minute: u32,
    /// Rate limit for large context requests (>200k tokens)
    /// More conservative to avoid API throttling
    pub large_context_requests_per_minute: u32,
    /// Enable tiered rate limiting (different limits for large context)
    pub tiered_enabled: bool,
}

impl RateLimitConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: super::helpers::env_or("RATE_LIMIT_ENABLED", "true") == "true",
            // Default to 50 RPM to match Gemini 3 Pro Tier 1 limit
            requests_per_minute: super::helpers::env_or("RATE_LIMIT_REQUESTS_PER_MINUTE", "50")
                .parse()
                .unwrap_or(50),
            // Large context gets half the rate limit by default
            large_context_requests_per_minute: super::helpers::env_or(
                "RATE_LIMIT_LARGE_CONTEXT_RPM",
                "25",
            )
            .parse()
            .unwrap_or(25),
            tiered_enabled: super::helpers::env_or("RATE_LIMIT_TIERED_ENABLED", "true") == "true",
        }
    }

    /// Get the appropriate rate limit based on context size
    pub fn get_rate_limit(&self, is_large_context: bool) -> u32 {
        if self.tiered_enabled && is_large_context {
            self.large_context_requests_per_minute
        } else {
            self.requests_per_minute
        }
    }
}
