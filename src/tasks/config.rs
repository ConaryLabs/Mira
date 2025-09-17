// src/tasks/config.rs

//! Configuration for background tasks

use std::time::Duration;

pub struct TaskConfig {
    // Analysis processor
    pub analysis_enabled: bool,
    pub analysis_interval: Duration,
    pub analysis_batch_size: usize,
    
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
            analysis_batch_size: 10,
            
            // Decay every 2 hours
            decay_enabled: std::env::var("TASK_DECAY_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            decay_interval: Duration::from_secs(
                std::env::var("TASK_DECAY_INTERVAL")
                    .unwrap_or_else(|_| "7200".to_string())
                    .parse()
                    .unwrap_or(7200)
            ),
            
            // Cleanup every hour
            cleanup_enabled: std::env::var("TASK_CLEANUP_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            cleanup_interval: Duration::from_secs(3600),
            session_max_age_hours: 168, // 7 days
            
            // Summary check every 5 minutes
            summary_processor_enabled: std::env::var("TASK_SUMMARY_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            summary_check_interval: Duration::from_secs(300),
        }
    }
}
