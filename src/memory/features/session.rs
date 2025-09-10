// src/services/memory/session.rs
// Session counter management with the critical increment fix

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Manages session-level state including message counters
/// This contains THE FIX for the counter that actually increments!
pub struct SessionManager {
    counters: Arc<RwLock<HashMap<String, usize>>>,
    metadata: Arc<RwLock<HashMap<String, SessionMetadata>>>,
}

/// Metadata for a session
#[derive(Debug, Clone)]
pub struct SessionMetadata {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
    pub total_messages: usize,
    pub total_summaries: usize,
    pub is_active: bool,
}

impl SessionManager {
    /// Creates a new session manager
    pub fn new() -> Self {
        Self {
            counters: Arc::new(RwLock::new(HashMap::new())),
            metadata: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Increments the message counter for a session
    /// THIS IS THE FIX - it actually increments the counter!
    pub async fn increment_counter(&self, session_id: &str) -> usize {
        let mut counters = self.counters.write().await;
        let counter = counters.entry(session_id.to_string()).or_insert(0);
        
        // THE FIX: Actually increment the value!
        *counter += 1;
        
        debug!("Incremented counter for session {} to {}", session_id, *counter);
        
        // Also update metadata
        self.update_last_activity(session_id).await;
        
        *counter
    }
    
    /// Gets the current message count for a session
    pub async fn get_message_count(&self, session_id: &str) -> usize {
        let counters = self.counters.read().await;
        *counters.get(session_id).unwrap_or(&0)
    }
    
    /// Resets the counter for a session
    pub async fn reset_counter(&self, session_id: &str) {
        let mut counters = self.counters.write().await;
        counters.insert(session_id.to_string(), 0);
        debug!("Reset counter for session {}", session_id);
    }
    
    /// Batch increments the counter (for bulk operations)
    pub async fn increment_by(&self, session_id: &str, amount: usize) -> usize {
        let mut counters = self.counters.write().await;
        let counter = counters.entry(session_id.to_string()).or_insert(0);
        *counter += amount;
        
        debug!("Incremented counter for session {} by {} to {}", 
               session_id, amount, *counter);
        
        self.update_last_activity(session_id).await;
        
        *counter
    }
    
    /// Updates last activity timestamp
    async fn update_last_activity(&self, session_id: &str) {
        let mut metadata = self.metadata.write().await;
        let now = chrono::Utc::now();
        
        metadata.entry(session_id.to_string())
            .and_modify(|m| {
                m.last_activity = now;
                m.total_messages += 1;
            })
            .or_insert_with(|| SessionMetadata {
                created_at: now,
                last_activity: now,
                total_messages: 1,
                total_summaries: 0,
                is_active: true,
            });
    }
    
    /// Increments the summary counter for a session
    pub async fn increment_summary_count(&self, session_id: &str) {
        let mut metadata = self.metadata.write().await;
        
        if let Some(meta) = metadata.get_mut(session_id) {
            meta.total_summaries += 1;
            info!("Session {} now has {} summaries", session_id, meta.total_summaries);
        }
    }
    
    /// Gets session metadata
    pub async fn get_metadata(&self, session_id: &str) -> Option<SessionMetadata> {
        let metadata = self.metadata.read().await;
        metadata.get(session_id).cloned()
    }
    
    /// Lists all active sessions
    pub async fn list_active_sessions(&self) -> Vec<(String, usize)> {
        let counters = self.counters.read().await;
        let metadata = self.metadata.read().await;
        
        counters.iter()
            .filter(|(id, _)| {
                metadata.get(*id)
                    .map(|m| m.is_active)
                    .unwrap_or(false)
            })
            .map(|(id, count)| (id.clone(), *count))
            .collect()
    }
    
    /// Marks a session as inactive
    pub async fn deactivate_session(&self, session_id: &str) {
        let mut metadata = self.metadata.write().await;
        
        if let Some(meta) = metadata.get_mut(session_id) {
            meta.is_active = false;
            debug!("Deactivated session {}", session_id);
        }
    }
    
    /// Cleans up old inactive sessions
    pub async fn cleanup_inactive_sessions(&self, max_age_hours: i64) -> usize {
        let now = chrono::Utc::now();
        let mut counters = self.counters.write().await;
        let mut metadata = self.metadata.write().await;
        
        let mut removed = 0;
        let sessions_to_remove: Vec<String> = metadata.iter()
            .filter(|(_, meta)| {
                !meta.is_active && 
                now.signed_duration_since(meta.last_activity).num_hours() > max_age_hours
            })
            .map(|(id, _)| id.clone())
            .collect();
        
        for session_id in sessions_to_remove {
            counters.remove(&session_id);
            metadata.remove(&session_id);
            removed += 1;
            debug!("Cleaned up inactive session {}", session_id);
        }
        
        if removed > 0 {
            info!("Cleaned up {} inactive sessions", removed);
        }
        
        removed
    }
    
    /// Gets statistics across all sessions
    pub async fn get_stats(&self) -> SessionStats {
        let counters = self.counters.read().await;
        let metadata = self.metadata.read().await;
        
        let total_sessions = counters.len();
        let active_sessions = metadata.values().filter(|m| m.is_active).count();
        let total_messages: usize = counters.values().sum();
        let total_summaries: usize = metadata.values().map(|m| m.total_summaries).sum();
        
        SessionStats {
            total_sessions,
            active_sessions,
            total_messages,
            total_summaries,
            average_messages_per_session: if total_sessions > 0 {
                total_messages as f32 / total_sessions as f32
            } else {
                0.0
            },
        }
    }
}

/// Statistics across all sessions
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub total_messages: usize,
    pub total_summaries: usize,
    pub average_messages_per_session: f32,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_session_counter_actually_increments() {
        let session_mgr = SessionManager::new();
        let session_id = "test_session";
        
        // THE TEST THAT PROVES THE FIX WORKS!
        let count1 = session_mgr.increment_counter(session_id).await;
        assert_eq!(count1, 1);
        
        let count2 = session_mgr.increment_counter(session_id).await;
        assert_eq!(count2, 2);
        
        let count3 = session_mgr.increment_counter(session_id).await;
        assert_eq!(count3, 3);
        
        // Verify get_message_count returns the same
        let current = session_mgr.get_message_count(session_id).await;
        assert_eq!(current, 3);
        
        // IT ACTUALLY WORKS!
    }
    
    #[tokio::test]
    async fn test_counter_reset() {
        let session_mgr = SessionManager::new();
        let session_id = "reset_test";
        
        // Increment a few times
        session_mgr.increment_counter(session_id).await;
        session_mgr.increment_counter(session_id).await;
        session_mgr.increment_counter(session_id).await;
        
        // Reset
        session_mgr.reset_counter(session_id).await;
        
        // Should be back to 0
        let count = session_mgr.get_message_count(session_id).await;
        assert_eq!(count, 0);
        
        // And should increment from 0
        let count = session_mgr.increment_counter(session_id).await;
        assert_eq!(count, 1);
    }
    
    #[tokio::test]
    async fn test_batch_increment() {
        let session_mgr = SessionManager::new();
        let session_id = "batch_test";
        
        // Batch increment by 10
        let count = session_mgr.increment_by(session_id, 10).await;
        assert_eq!(count, 10);
        
        // Regular increment should add 1
        let count = session_mgr.increment_counter(session_id).await;
        assert_eq!(count, 11);
    }
    
    #[tokio::test]
    async fn test_multiple_sessions() {
        let session_mgr = SessionManager::new();
        
        // Session 1
        session_mgr.increment_counter("session1").await;
        session_mgr.increment_counter("session1").await;
        
        // Session 2
        session_mgr.increment_counter("session2").await;
        session_mgr.increment_counter("session2").await;
        session_mgr.increment_counter("session2").await;
        
        // Session 3
        session_mgr.increment_counter("session3").await;
        
        // Verify independent counters
        assert_eq!(session_mgr.get_message_count("session1").await, 2);
        assert_eq!(session_mgr.get_message_count("session2").await, 3);
        assert_eq!(session_mgr.get_message_count("session3").await, 1);
    }
    
    #[tokio::test]
    async fn test_session_metadata() {
        let session_mgr = SessionManager::new();
        let session_id = "meta_test";
        
        // First message creates metadata
        session_mgr.increment_counter(session_id).await;
        
        let meta = session_mgr.get_metadata(session_id).await.unwrap();
        assert_eq!(meta.total_messages, 1);
        assert_eq!(meta.total_summaries, 0);
        assert!(meta.is_active);
        
        // Add a summary
        session_mgr.increment_summary_count(session_id).await;
        
        let meta = session_mgr.get_metadata(session_id).await.unwrap();
        assert_eq!(meta.total_summaries, 1);
    }
}
