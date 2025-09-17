// src/tasks/config.rs
// Configuration for background tasks

use std::time::Duration;

pub struct TaskConfig {
    // Analysis processor
    pub analysis_enabled: bool,
    pub analysis_interval: Duration,
    pub analysis_batch_size: usize,
    
    // Decay scheduler - updated defaults
    pub decay_enabled: bool,
    pub decay_interval: Duration,
    pub decay_batch_size: usize,
    
    // Session cleanup
    pub cleanup_enabled: bool,
    pub cleanup_interval: Duration,
    pub session_max_age_hours: i64,
    
    // Summary processor
    pub summary_processor_enabled: bool,
    pub summary_check_interval: Duration,
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
                    .unwrap_or(10)
            ),
            analysis_batch_size: std::env::var("TASK_ANALYSIS_BATCH_SIZE")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .unwrap_or(10),
            
            // Decay every 4 hours (was 2, now more reasonable)
            decay_enabled: std::env::var("TASK_DECAY_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            decay_interval: Duration::from_secs(
                std::env::var("TASK_DECAY_INTERVAL")
                    .unwrap_or_else(|_| "14400".to_string())  // 4 hours
                    .parse()
                    .unwrap_or(14400)
            ),
            decay_batch_size: std::env::var("TASK_DECAY_BATCH_SIZE")
                .unwrap_or_else(|_| "1000".to_string())
                .parse()
                .unwrap_or(1000),
            
            // Cleanup every hour
            cleanup_enabled: std::env::var("TASK_CLEANUP_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            cleanup_interval: Duration::from_secs(
                std::env::var("TASK_CLEANUP_INTERVAL")
                    .unwrap_or_else(|_| "3600".to_string())
                    .parse()
                    .unwrap_or(3600)
            ),
            session_max_age_hours: std::env::var("TASK_SESSION_MAX_AGE_HOURS")
                .unwrap_or_else(|_| "168".to_string())
                .parse()
                .unwrap_or(168),  // 7 days
            
            // Summary check every 5 minutes
            summary_processor_enabled: std::env::var("TASK_SUMMARY_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            summary_check_interval: Duration::from_secs(
                std::env::var("TASK_SUMMARY_INTERVAL")
                    .unwrap_or_else(|_| "300".to_string())
                    .parse()
                    .unwrap_or(300)
            ),
        }
    }
    
    /// Get a human-readable summary of the configuration
    pub fn summary(&self) -> String {
        format!(
            "Tasks Config:\n\
            - Analysis: {} (every {} secs, batch: {})\n\
            - Decay: {} (every {} hours, batch: {})\n\
            - Cleanup: {} (every {} min, max age: {} days)\n\
            - Summaries: {} (every {} min)",
            if self.analysis_enabled { "ON" } else { "OFF" },
            self.analysis_interval.as_secs(),
            self.analysis_batch_size,
            if self.decay_enabled { "ON" } else { "OFF" },
            self.decay_interval.as_secs() / 3600,
            self.decay_batch_size,
            if self.cleanup_enabled { "ON" } else { "OFF" },
            self.cleanup_interval.as_secs() / 60,
            self.session_max_age_hours / 24,
            if self.summary_processor_enabled { "ON" } else { "OFF" },
            self.summary_check_interval.as_secs() / 60,
        )
    }
}
