// src/config/caching.rs
// Caching configuration

use serde::{Deserialize, Serialize};

/// Request cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestCacheConfig {
    pub enabled: bool,
    pub ttl_seconds: u64,
}

impl RequestCacheConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: super::helpers::require_env_parsed("MIRA_ENABLE_REQUEST_CACHE"),
            ttl_seconds: super::helpers::require_env_parsed("MIRA_CACHE_TTL_SECONDS"),
        }
    }
}

/// Recent message cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentCacheConfig {
    pub enabled: bool,
    pub capacity: usize,
    pub ttl_seconds: u64,
    pub max_per_session: usize,
    pub warmup: bool,
}

impl RecentCacheConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: super::helpers::require_env_parsed("MIRA_ENABLE_RECENT_CACHE"),
            capacity: super::helpers::require_env_parsed("MIRA_RECENT_CACHE_CAPACITY"),
            ttl_seconds: super::helpers::require_env_parsed("MIRA_RECENT_CACHE_TTL"),
            max_per_session: super::helpers::require_env_parsed(
                "MIRA_RECENT_CACHE_MAX_PER_SESSION",
            ),
            warmup: super::helpers::require_env_parsed("MIRA_RECENT_CACHE_WARMUP"),
        }
    }
}

/// API retry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_retries: usize,
    pub retry_delay_ms: u64,
}

impl RetryConfig {
    pub fn from_env() -> Self {
        Self {
            max_retries: super::helpers::require_env_parsed("MIRA_API_MAX_RETRIES"),
            retry_delay_ms: super::helpers::require_env_parsed("MIRA_API_RETRY_DELAY_MS"),
        }
    }
}
