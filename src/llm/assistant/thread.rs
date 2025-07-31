// src/llm/assistant/thread.rs - Updated for Responses API

use crate::llm::client::OpenAIClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use super::manager::ResponseMessage;

/// Since threads are deprecated, we manage conversation context ourselves
pub struct ThreadManager {
    /// Maps session_id -> conversation history
    conversations: Arc<RwLock<HashMap<String, Vec<ResponseMessage>>>>,
    /// Maps session_id -> vector_store_ids
    session_stores: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl ThreadManager {
    pub fn new(_client: Arc<OpenAIClient>) -> Self {
        Self {
            conversations: Arc::new(RwLock::new(HashMap::new())),
            session_stores: Arc::new(RwLock::new(HashMap::new())),
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
    
    /// Get or create a thread with vector store tools attached
    pub async fn get_or_create_thread_with_tools(
        &self,
        session_id: &str,
        vector_store_ids: Vec<String>,
    ) -> Result<String> {
        // First ensure conversation exists
        let thread_id = self.get_or_create_thread(session_id).await?;
        
        // Store the vector store IDs for this session
        let mut session_stores = self.session_stores.write().await;
        session_stores.insert(session_id.to_string(), vector_store_ids);
        
        eprintln!("📎 Attached vector stores to session: {}", session_id);
        
        Ok(thread_id)
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
    
    /// Get vector stores for a session
    pub async fn get_session_stores(&self, session_id: &str) -> Vec<String> {
        let session_stores = self.session_stores.read().await;
        session_stores.get(session_id).cloned().unwrap_or_default()
    }
    
    /// Clear thread for a session (useful for starting fresh)
    pub async fn clear_thread(&self, session_id: &str) -> Result<()> {
        let mut conversations = self.conversations.write().await;
        conversations.remove(session_id);
        
        let mut session_stores = self.session_stores.write().await;
        session_stores.remove(session_id);
        
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
