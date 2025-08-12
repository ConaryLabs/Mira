// src/llm/responses/thread.rs
//! Minimal in‑memory thread manager for chat history.
//! Provides ResponseMessage and a per-session store with FIFO semantics.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ResponseMessage {
    pub role: String,            // "user" | "assistant"
    pub content: Option<String>, // text only for now
}

#[derive(Clone, Default)]
pub struct ThreadManager {
    inner: Arc<RwLock<HashMap<String, VecDeque<ResponseMessage>>>>,
}

impl ThreadManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a message to a session's conversation.
    pub async fn add_message(&self, session_id: &str, msg: ResponseMessage) -> anyhow::Result<()> {
        let mut guard = self.inner.write().await;
        guard.entry(session_id.to_string())
            .or_insert_with(VecDeque::new)
            .push_back(msg);
        Ok(())
    }

    /// Get the full conversation for a session (caller can truncate).
    pub async fn get_conversation(&self, session_id: &str) -> Vec<ResponseMessage> {
        let guard = self.inner.read().await;
        guard.get(session_id)
            .map(|v| v.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Optional helper to cap messages.
    pub async fn get_conversation_capped(&self, session_id: &str, cap: usize) -> Vec<ResponseMessage> {
        let mut all = self.get_conversation(session_id).await;
        if all.len() > cap {
            let start = all.len() - cap;
            all = all.split_off(start);
        }
        all
    }
}
