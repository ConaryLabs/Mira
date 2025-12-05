// src/config/caching.rs
// Caching configuration

use serde::{Deserialize, Serialize};

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
