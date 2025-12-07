// src/memory/features/summarization/triggers/background_triggers.rs

use crate::memory::features::memory_types::SummaryType;
use tracing::{debug, info};

/// Handles background task trigger logic for summaries
pub struct BackgroundTriggers;

impl BackgroundTriggers {
    pub fn new() -> Self {
        Self
    }

    /// Determines if summary should be triggered based on message count
    /// Rolling summaries are created every 100 messages
    pub fn should_create_summary(&self, message_count: usize) -> Option<SummaryType> {
        if message_count > 0 && message_count % 100 == 0 {
            info!(
                "Background trigger: Creating rolling summary at count {}",
                message_count
            );
            return Some(SummaryType::Rolling);
        }

        debug!("No summary trigger at message count {}", message_count);
        None
    }

    /// Check if enough time has passed since last summary (future enhancement)
    pub fn should_create_time_based_summary(
        &self,
        _last_summary_time: Option<chrono::DateTime<chrono::Utc>>,
    ) -> bool {
        false
    }
}
