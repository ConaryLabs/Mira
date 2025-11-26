// src/tasks/config.rs
// Configuration for background tasks

use std::time::Duration;

pub struct TaskConfig {
    // Analysis processor
    pub analysis_enabled: bool,
    pub analysis_interval: Duration,

    // Decay scheduler
    pub decay_enabled: bool,
    pub decay_interval: Duration,

    // Session cleanup
    pub cleanup_enabled: bool,
    pub cleanup_interval: Duration,
    pub session_max_age_hours: i64,

    // Summary processor
    pub summary_processor_enabled: bool,
    pub summary_check_interval: Duration,

    // Code sync (Layer 2: Background parsing)
    pub code_sync_enabled: bool,
    pub code_sync_interval: Duration,

    // Embedding cleanup (orphaned Qdrant entries)
    pub embedding_cleanup_enabled: bool,
    pub embedding_cleanup_interval: Duration,

    // File watcher (real-time file change detection)
    pub file_watcher_enabled: bool,

    // Active session processing limit
    pub active_session_limit: i64,
}

impl TaskConfig {
    pub fn from_env() -> Self {
        Self {
            // Analysis every 10 seconds
            analysis_enabled: std::env::var("TASK_ANALYSIS_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            analysis_interval: Duration::from_secs(
                std::env::var("TASK_ANALYSIS_INTERVAL")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()
                    .unwrap_or(10),
            ),

            // Decay every 4 hours
            decay_enabled: std::env::var("TASK_DECAY_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            decay_interval: Duration::from_secs(
                std::env::var("TASK_DECAY_INTERVAL")
                    .unwrap_or_else(|_| "14400".to_string()) // 4 hours
                    .parse()
                    .unwrap_or(14400),
            ),

            // Cleanup every hour
            cleanup_enabled: std::env::var("TASK_CLEANUP_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            cleanup_interval: Duration::from_secs(
                std::env::var("TASK_CLEANUP_INTERVAL")
                    .unwrap_or_else(|_| "3600".to_string())
                    .parse()
                    .unwrap_or(3600),
            ),
            session_max_age_hours: std::env::var("SESSION_MAX_AGE_HOURS")
                .unwrap_or_else(|_| "168".to_string()) // 7 days
                .parse()
                .unwrap_or(168),

            // Summary check every 30 minutes
            summary_processor_enabled: std::env::var("TASK_SUMMARY_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            summary_check_interval: Duration::from_secs(
                std::env::var("TASK_SUMMARY_INTERVAL")
                    .unwrap_or_else(|_| "1800".to_string())
                    .parse()
                    .unwrap_or(1800),
            ),

            // Code sync every 5 minutes (Layer 2: Safety net for external changes)
            code_sync_enabled: std::env::var("TASK_CODE_SYNC_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            code_sync_interval: Duration::from_secs(
                std::env::var("TASK_CODE_SYNC_INTERVAL")
                    .unwrap_or_else(|_| "300".to_string()) // 5 minutes
                    .parse()
                    .unwrap_or(300),
            ),

            // Embedding cleanup every 7 days (weekly orphan removal)
            embedding_cleanup_enabled: std::env::var("TASK_EMBEDDING_CLEANUP_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            embedding_cleanup_interval: Duration::from_secs(
                std::env::var("TASK_EMBEDDING_CLEANUP_INTERVAL")
                    .unwrap_or_else(|_| "604800".to_string()) // 7 days
                    .parse()
                    .unwrap_or(604800),
            ),

            // File watcher enabled by default
            // When enabled, code_sync polling can be reduced or disabled
            file_watcher_enabled: std::env::var("TASK_FILE_WATCHER_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),

            // Limit active sessions to avoid overload
            active_session_limit: std::env::var("ACTIVE_SESSION_LIMIT")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .unwrap_or(100),
        }
    }
}
