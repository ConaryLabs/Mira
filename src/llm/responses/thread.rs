// src/llm/responses/thread.rs
//! Minimal in-memory thread manager for chat history.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ResponseMessage {
    pub role: String,
    pub content: Option<String>,
}

impl ResponseMessage {
    /// Create a user message
    pub fn user(content: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.to_string()),
        }
    }
    
    /// Create an assistant message
    pub fn assistant(content: &str) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(content.to_string()),
        }
    }
}

#[derive(Clone, Default)]
pub struct ThreadManager {
    inner: Arc<RwLock<HashMap<String, VecDeque<ResponseMessage>>>>,
    threads: Arc<RwLock<HashMap<String, String>>>, // session_id -> thread_id mapping
}

impl ThreadManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a thread for the given session ID
    pub async fn get_or_create_thread(&self, session_id: &str) -> anyhow::Result<String> {
        let mut threads = self.threads.write().await;
        
        if let Some(thread_id) = threads.get(session_id) {
            Ok(thread_id.clone())
        } else {
            let thread_id = Uuid::new_v4().to_string();
            threads.insert(session_id.to_string(), thread_id.clone());
            Ok(thread_id)
        }
    }

    /// Append a message to a session's conversation
    pub async fn add_message(&self, session_id: &str, msg: ResponseMessage) -> anyhow::Result<()> {
        let mut guard = self.inner.write().await;
        guard.entry(session_id.to_string())
            .or_insert_with(VecDeque::new)
            .push_back(msg);
        Ok(())
    }

    /// Get the full conversation for a session
    pub async fn get_conversation(&self, session_id: &str) -> Vec<ResponseMessage> {
        let guard = self.inner.read().await;
        guard.get(session_id)
            .map(|v| v.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get conversation with a cap on number of messages
    pub async fn get_conversation_capped(&self, session_id: &str, cap: usize) -> Vec<ResponseMessage> {
        let mut all = self.get_conversation(session_id).await;
        if all.len() > cap {
            let start = all.len() - cap;
            all = all.split_off(start);
        }
        all
    }
}
