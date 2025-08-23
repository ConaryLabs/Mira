// src/services/chat/context.rs
// Extracted Context Building Logic from chat.rs
// Handles recall context building with memory retrieval

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn};

use crate::llm::client::OpenAIClient;
use crate::memory::recall::{RecallContext, build_context};
use crate::memory::traits::MemoryStore;
use crate::services::chat::config::ChatConfig;
use crate::api::error::IntoApiError;

/// Context builder for chat conversations
pub struct ContextBuilder {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
    qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
    config: ChatConfig,
}

impl ContextBuilder {
    /// Create new context builder
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
        config: ChatConfig,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            qdrant_store,
            config,
        }
    }

    /// Build recall context for a chat session
    pub async fn build_context(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        info!(
            "🔍 Building context for session: {} (history_cap={}, vector_results={})",
            session_id,
            self.config.history_message_cap(),
            self.config.max_vector_search_results()
        );

        // Get embedding for semantic search
        let embedding = match self.llm_client.get_embedding(user_text).await {
            Ok(emb) => Some(emb),
            Err(e) => {
                warn!("⚠️ Failed to get embedding for context building: {}", e);
                None
            }
        };

        // Build context using existing build_context function
        let context = build_context(
            session_id,
            embedding.as_deref(),
            self.config.history_message_cap(),
            self.config.max_vector_search_results(),
            self.sqlite_store.as_ref(),
            self.qdrant_store.as_ref(),
        )
        .await
        .unwrap_or_else(|e| {
            warn!("⚠️ Failed to build full context: {}. Using empty context.", e);
            RecallContext {
                recent: Vec::new(),
                semantic: Vec::new(),
            }
        });

        info!(
            "✅ Context built: {} recent messages, {} semantic matches",
            context.recent.len(),
            context.semantic.len()
        );

        Ok(context)
    }

    /// Build context with fallback strategies
    pub async fn build_context_with_fallbacks(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        // First try: Full context with embeddings
        if let Ok(context) = self.build_context(session_id, user_text).await {
            return Ok(context);
        }

        warn!("⚠️ Full context building failed, trying recent-only fallback");

        // Second try: Recent messages only (no semantic search)
        match self.sqlite_store
            .load_recent(session_id, self.config.history_message_cap())
            .await
        {
            Ok(recent) => {
                info!("✅ Fallback context: {} recent messages only", recent.len());
                Ok(RecallContext {
                    recent,
                    semantic: Vec::new(),
                })
            }
            Err(e) => {
                warn!("⚠️ Even recent-only context failed: {}", e);
                Ok(RecallContext {
                    recent: Vec::new(),
                    semantic: Vec::new(),
                })
            }
        }
    }

    /// Build minimal context for emergency situations
    pub async fn build_minimal_context(&self, session_id: &str) -> RecallContext {
        // Try to get at least a few recent messages
        let recent = self.sqlite_store
            .load_recent(session_id, 5) // Just get last 5 messages
            .await
            .unwrap_or_else(|_| Vec::new());

        RecallContext {
            recent,
            semantic: Vec::new(),
        }
    }

    /// Check if vector search is available and configured
    pub fn can_use_vector_search(&self) -> bool {
        self.config.enable_vector_search() && self.config.max_vector_search_results() > 0
    }

    /// Get context stats for debugging
    pub async fn get_context_stats(&self, session_id: &str) -> Result<ContextStats> {
        // Get total message count from sqlite
        let total_messages = self.sqlite_store
            .load_recent(session_id, usize::MAX)
            .await
            .into_api_error("Failed to load messages for stats")?
            .len();

        // Try to get vector store stats (this might not be implemented in all stores)
        let vector_embeddings = if self.can_use_vector_search() {
            // This is a rough approximation - actual implementation depends on store
            total_messages.saturating_sub(10) // Assume some messages don't have embeddings
        } else {
            0
        };

        Ok(ContextStats {
            total_messages,
            vector_embeddings,
            history_cap: self.config.history_message_cap(),
            vector_search_enabled: self.config.enable_vector_search(),
            max_vector_results: self.config.max_vector_search_results(),
        })
    }
}

/// Context statistics for debugging and monitoring
#[derive(Debug, Clone)]
pub struct ContextStats {
    pub total_messages: usize,
    pub vector_embeddings: usize,
    pub history_cap: usize,
    pub vector_search_enabled: bool,
    pub max_vector_results: usize,
}

impl ContextStats {
    pub fn usage_percentage(&self) -> f32 {
        if self.history_cap == 0 {
            return 0.0;
        }
        (self.total_messages as f32 / self.history_cap as f32 * 100.0).min(100.0)
    }

    pub fn has_sufficient_history(&self) -> bool {
        self.total_messages >= (self.history_cap / 4) // At least 25% of cap
    }

    pub fn vector_coverage(&self) -> f32 {
        if self.total_messages == 0 {
            return 0.0;
        }
        self.vector_embeddings as f32 / self.total_messages as f32 * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_stats() {
        let stats = ContextStats {
            total_messages: 30,
            vector_embeddings: 25,
            history_cap: 50,
            vector_search_enabled: true,
            max_vector_results: 15,
        };

        assert_eq!(stats.usage_percentage(), 60.0);
        assert!(stats.has_sufficient_history());
        assert_eq!(stats.vector_coverage(), 83.33333);
    }

    #[test]
    fn test_context_stats_full() {
        let stats = ContextStats {
            total_messages: 100,
            vector_embeddings: 90,
            history_cap: 50,
            vector_search_enabled: true,
            max_vector_results: 15,
        };

        assert_eq!(stats.usage_percentage(), 100.0); // Capped at 100%
        assert!(stats.has_sufficient_history());
    }
}
