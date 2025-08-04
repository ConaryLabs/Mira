// src/llm/assistant/thread.rs - Modernized for OpenAI Responses API (no tools, no file_search)

use crate::llm::client::OpenAIClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use super::manager::ResponseMessage;

/// Manages per-session conversation context.
/// No threads or tool management anymore; pure conversation memory.
pub struct ThreadManager {
    /// Maps session_id -> conversation history
    conversations: Arc<RwLock<HashMap<String, Vec<ResponseMessage>>>>,
}

impl ThreadManager {
    pub fn new(_client: Arc<OpenAIClient>) -> Self {
        Self {
            conversations: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Get or create a "thread" (now just a conversation history)
    pub async fn get_or_create_thread(&self, session_id: &str) -> Result<String> {
        let mut conversations = self.conversations.write().await;
        
        if !conversations.contains_key(session_id) {
            conversations.insert(session_id.to_string(), Vec::new());
            eprintln!("📝 Created new conversation for session: {}", session_id);
        }
        
        // Return the session_id as the "thread_id"
        Ok(session_id.to_string())
    }
    
    /// Add a message to the conversation
    pub async fn add_message(&self, session_id: &str, message: ResponseMessage) -> Result<()> {
        let mut conversations = self.conversations.write().await;
        
        if let Some(history) = conversations.get_mut(session_id) {
            history.push(message);
            
            // Keep only last 20 messages to avoid context length issues
            if history.len() > 20 {
                history.drain(0..history.len() - 20);
            }
        }
        
        Ok(())
    }
    
    /// Get conversation history
    pub async fn get_conversation(&self, session_id: &str) -> Vec<ResponseMessage> {
        let conversations = self.conversations.read().await;
        conversations.get(session_id).cloned().unwrap_or_default()
    }
    
    /// Clear thread for a session (useful for starting fresh)
    pub async fn clear_thread(&self, session_id: &str) -> Result<()> {
        let mut conversations = self.conversations.write().await;
        conversations.remove(session_id);
        Ok(())
    }
    
    /// List all active threads
    pub async fn list_threads(&self) -> Vec<(String, String)> {
        let conversations = self.conversations.read().await;
        conversations.keys()
            .map(|k| (k.clone(), k.clone())) // session_id is the thread_id now
            .collect()
    }
}
