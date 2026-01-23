// crates/mira-server/src/context/analytics.rs
// Analytics and learning for context injection

use crate::db::pool::DatabasePool;
use crate::db::set_server_state_sync;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::InjectionSource;

/// Event recorded when context is injected
#[derive(Debug, Clone)]
pub struct InjectionEvent {
    pub session_id: String,
    pub project_id: Option<i64>,
    pub sources: Vec<InjectionSource>,
    pub context_len: usize,
    pub message_preview: String,
}

/// In-memory analytics for injection events
/// Persists summary stats to database periodically
pub struct InjectionAnalytics {
    pool: Arc<DatabasePool>,
    events: Mutex<Vec<InjectionEvent>>,
    total_injections: Mutex<u64>,
    total_chars: Mutex<u64>,
}

impl InjectionAnalytics {
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            events: Mutex::new(Vec::new()),
            total_injections: Mutex::new(0),
            total_chars: Mutex::new(0),
        }
    }

    /// Record an injection event
    pub async fn record(&self, event: InjectionEvent) {
        let context_len = event.context_len;

        // Update counters
        {
            let mut total = self.total_injections.lock().await;
            *total += 1;
        }
        {
            let mut chars = self.total_chars.lock().await;
            *chars += context_len as u64;
        }

        // Store event (keep last 100)
        {
            let mut events = self.events.lock().await;
            events.push(event);
            if events.len() > 100 {
                events.remove(0);
            }
        }

        // Persist stats every 10 injections
        let count = *self.total_injections.lock().await;
        if count % 10 == 0 {
            self.persist_stats().await;
        }
    }

    /// Persist stats to database
    async fn persist_stats(&self) {
        let total = *self.total_injections.lock().await;
        let chars = *self.total_chars.lock().await;

        let total_str = total.to_string();
        let chars_str = chars.to_string();

        if let Err(e) = self.pool.interact(move |conn| {
            set_server_state_sync(conn, "injection_total_count", &total_str)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            set_server_state_sync(conn, "injection_total_chars", &chars_str)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok::<_, anyhow::Error>(())
        }).await {
            tracing::debug!("Failed to persist injection stats: {}", e);
        }
    }

    /// Get analytics summary
    pub async fn summary(&self, _project_id: Option<i64>) -> String {
        let total = *self.total_injections.lock().await;
        let chars = *self.total_chars.lock().await;
        let events = self.events.lock().await;

        if total == 0 {
            return "No context injections recorded yet.".to_string();
        }

        let avg_chars = if total > 0 { chars / total } else { 0 };

        // Count source usage
        let mut semantic_count = 0u64;
        let mut file_count = 0u64;
        let mut task_count = 0u64;

        for event in events.iter() {
            for source in &event.sources {
                match source {
                    InjectionSource::Semantic => semantic_count += 1,
                    InjectionSource::FileAware => file_count += 1,
                    InjectionSource::TaskAware => task_count += 1,
                }
            }
        }

        format!(
            "Injection analytics:\n  Total: {} injections, {} chars ({} avg)\n  Sources: semantic={}, files={}, tasks={}",
            total, chars, avg_chars, semantic_count, file_count, task_count
        )
    }

    /// Mark that injected context was useful (e.g., user acted on it)
    /// This can be used for learning which contexts are valuable
    pub async fn mark_useful(&self, session_id: &str) {
        // For now, just log - future: update weights for injection strategies
        tracing::debug!("Context injection marked useful for session {}", session_id);
    }

    /// Get recent events for debugging
    pub async fn recent_events(&self, limit: usize) -> Vec<InjectionEvent> {
        let events = self.events.lock().await;
        events.iter().rev().take(limit).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_analytics_record() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let analytics = InjectionAnalytics::new(pool);

        analytics.record(InjectionEvent {
            session_id: "test-session".to_string(),
            project_id: Some(1),
            sources: vec![InjectionSource::Semantic],
            context_len: 100,
            message_preview: "test message".to_string(),
        }).await;

        let summary = analytics.summary(None).await;
        assert!(summary.contains("1 injections"));
        assert!(summary.contains("100 chars"));
    }

    #[tokio::test]
    async fn test_analytics_summary_empty() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let analytics = InjectionAnalytics::new(pool);

        let summary = analytics.summary(None).await;
        assert!(summary.contains("No context injections"));
    }

    #[tokio::test]
    async fn test_recent_events() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let analytics = InjectionAnalytics::new(pool);

        for i in 0..5 {
            analytics.record(InjectionEvent {
                session_id: format!("session-{}", i),
                project_id: None,
                sources: vec![],
                context_len: i * 10,
                message_preview: format!("message {}", i),
            }).await;
        }

        let recent = analytics.recent_events(3).await;
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].session_id, "session-4"); // Most recent first
    }
}
