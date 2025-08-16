// src/llm/responses/thread.rs
// Updated for GPT-5 Responses API - August 15, 2025
// Changes:
// - Added previous_response_id tracking for conversation continuity
// - Enhanced session management with response ID history
// - Improved token counting and message trimming
// - Fixed visibility of SessionInfo for summarization service

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Message in a conversation thread
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

/// Session information including response ID tracking
#[derive(Debug, Clone)]
pub struct SessionInfo { // --- FIXED: Made this struct public ---
    pub messages: VecDeque<ResponseMessage>,
    pub previous_response_id: Option<String>,
    pub response_id_history: VecDeque<String>,
    pub total_tokens: usize,
    // pub created_at: chrono::DateTime<chrono::Utc>, // Removed to clear dead_code warning
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// Manages conversation threads locally with response ID tracking
pub struct ThreadManager {
    pub sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
    max_messages_per_session: usize,
    max_response_id_history: usize,
    token_limit: usize,
}

impl ThreadManager {
    /// Create a new ThreadManager
    pub fn new(max_messages: usize, token_limit: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_messages_per_session: max_messages,
            max_response_id_history: 10,
            token_limit,
        }
    }

    // Removed unused get_or_create_session method to clear dead_code warning

    /// Add a message to a session
    pub async fn add_message(
        &self,
        session_id: &str,
        message: ResponseMessage,
    ) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| {
                info!("📝 Creating new session: {}", session_id);
                SessionInfo {
                    messages: VecDeque::new(),
                    previous_response_id: None,
                    response_id_history: VecDeque::new(),
                    total_tokens: 0,
                    // created_at: chrono::Utc::now(),
                    last_activity: chrono::Utc::now(),
                }
            });

        session.messages.push_back(message.clone());
        session.last_activity = chrono::Utc::now();

        if let Some(content) = &message.content {
            session.total_tokens += content.len() / 4;
        }

        self.trim_session_messages(session);

        debug!(
            "Added message to session {}: {} messages, ~{} tokens",
            session_id,
            session.messages.len(),
            session.total_tokens
        );

        Ok(())
    }

    /// Update the previous_response_id for a session
    pub async fn update_response_id(
        &self,
        session_id: &str,
        response_id: String,
    ) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        
        if let Some(session) = sessions.get_mut(session_id) {
            session.previous_response_id = Some(response_id.clone());
            
            session.response_id_history.push_back(response_id.clone());
            
            while session.response_id_history.len() > self.max_response_id_history {
                session.response_id_history.pop_front();
            }
            
            session.last_activity = chrono::Utc::now();
            
            debug!(
                "Updated response ID for session {}: {}",
                session_id, response_id
            );
        } else {
            let mut session = SessionInfo {
                messages: VecDeque::new(),
                previous_response_id: Some(response_id.clone()),
                response_id_history: VecDeque::new(),
                total_tokens: 0,
                // created_at: chrono::Utc::now(),
                last_activity: chrono::Utc::now(),
            };
            session.response_id_history.push_back(response_id);
            sessions.insert(session_id.to_string(), session);
        }
        
        Ok(())
    }

    /// Get the previous_response_id for a session
    pub async fn get_previous_response_id(&self, session_id: &str) -> Option<String> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .and_then(|s| s.previous_response_id.clone())
    }

    /// Get conversation history with a message cap
    pub async fn get_conversation_capped(
        &self,
        session_id: &str,
        max_messages: usize,
    ) -> Vec<ResponseMessage> {
        let sessions = self.sessions.read().await;
        
        if let Some(session) = sessions.get(session_id) {
            let messages: Vec<ResponseMessage> = session.messages.iter().cloned().collect();
            
            let start = messages.len().saturating_sub(max_messages);
            messages[start..].to_vec()
        } else {
            Vec::new()
        }
    }

    /// Get full conversation history
    pub async fn get_full_conversation(&self, session_id: &str) -> Vec<ResponseMessage> {
        let sessions = self.sessions.read().await;
        
        if let Some(session) = sessions.get(session_id) {
            session.messages.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Get conversation with token limit
    pub async fn get_conversation_with_token_limit(
        &self,
        session_id: &str,
        token_limit: usize,
    ) -> Vec<ResponseMessage> {
        let sessions = self.sessions.read().await;
        
        if let Some(session) = sessions.get(session_id) {
            let mut result = Vec::new();
            let mut token_count = 0;
            
            for message in session.messages.iter().rev() {
                let message_tokens = self.estimate_message_tokens(message);
                
                if token_count + message_tokens > token_limit {
                    break;
                }
                
                result.push(message.clone());
                token_count += message_tokens;
            }
            
            result.reverse();
            result
        } else {
            Vec::new()
        }
    }

    /// Clear a session
    pub async fn clear_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        
        if let Some(session) = sessions.get_mut(session_id) {
            session.messages.clear();
            session.previous_response_id = None;
            session.response_id_history.clear();
            session.total_tokens = 0;
            session.last_activity = chrono::Utc::now();
            info!("🧹 Cleared session: {}", session_id);
        }
        
        Ok(())
    }

    /// Delete a session entirely
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        info!("🗑️ Deleted session: {}", session_id);
        Ok(())
    }

    /// List all active sessions
    pub async fn list_sessions(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }

    /// Clean up old sessions (older than specified hours)
    pub async fn cleanup_old_sessions(&self, max_age_hours: i64) -> usize {
        let mut sessions = self.sessions.write().await;
        let now = chrono::Utc::now();
        let cutoff = now - chrono::Duration::hours(max_age_hours);
        
        let old_sessions: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| session.last_activity < cutoff)
            .map(|(id, _)| id.clone())
            .collect();
        
        let count = old_sessions.len();
        for session_id in old_sessions {
            sessions.remove(&session_id);
            debug!("Cleaned up old session: {}", session_id);
        }
        
        if count > 0 {
            info!("🧹 Cleaned up {} old sessions", count);
        }
        
        count
    }

    /// Trim messages in a session to stay within limits
    fn trim_session_messages(&self, session: &mut SessionInfo) {
        while session.messages.len() > self.max_messages_per_session {
            if let Some(removed) = session.messages.pop_front() {
                if let Some(content) = &removed.content {
                    session.total_tokens = session.total_tokens.saturating_sub(content.len() / 4);
                }
            }
        }

        while session.total_tokens > self.token_limit && !session.messages.is_empty() {
            if let Some(removed) = session.messages.pop_front() {
                if let Some(content) = &removed.content {
                    session.total_tokens = session.total_tokens.saturating_sub(content.len() / 4);
                }
            }
        }
    }

    /// Estimate tokens for a message (rough approximation)
    fn estimate_message_tokens(&self, message: &ResponseMessage) -> usize {
        let mut tokens = 0;
        
        if let Some(content) = &message.content {
            tokens += content.len() / 4;
        }
        
        tokens += 10;
        
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_response_id_tracking() {
        let manager = ThreadManager::new(100, 10000);
        let session_id = "test_session";
        
        let message = ResponseMessage {
            role: "user".to_string(),
            content: Some("Hello".to_string()),
            name: None,
            function_call: None,
            tool_calls: None,
        };
        
        manager.add_message(session_id, message).await.unwrap();
        
        manager.update_response_id(session_id, "resp_123".to_string()).await.unwrap();
        
        let prev_id = manager.get_previous_response_id(session_id).await;
        assert_eq!(prev_id, Some("resp_123".to_string()));
        
        manager.update_response_id(session_id, "resp_456".to_string()).await.unwrap();
        
        let prev_id = manager.get_previous_response_id(session_id).await;
        assert_eq!(prev_id, Some("resp_456".to_string()));
    }
}
