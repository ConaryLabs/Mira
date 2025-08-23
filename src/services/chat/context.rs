// src/services/chat/context.rs

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn};

use crate::llm::client::OpenAIClient;
use crate::memory::recall::{RecallContext, build_context};
use crate::memory::traits::MemoryStore;
use crate::services::chat::config::ChatConfig;
use crate::api::error::IntoApiError;

#[derive(Debug)]
pub struct ContextStats {
    pub total_messages: usize,
    pub recent_messages: usize,
    pub semantic_matches: usize,
}

#[derive(Clone)]
pub struct ContextBuilder {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
    qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
    config: ChatConfig,
}

impl ContextBuilder {
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

    pub fn sqlite_store(&self) -> &Arc<dyn MemoryStore + Send + Sync> {
        &self.sqlite_store
    }

    pub fn qdrant_store(&self) -> &Arc<dyn MemoryStore + Send + Sync> {
        &self.qdrant_store
    }

    pub fn config(&self) -> &ChatConfig {
        &self.config
    }

    pub async fn build_context(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        info!(
            "Building context for session: {} (history_cap={}, vector_results={})",
            session_id,
            self.config.history_message_cap(),
            self.config.max_vector_search_results()
        );

        let embedding = match self.llm_client.get_embedding(user_text).await {
            Ok(emb) => Some(emb),
            Err(e) => {
                warn!("Failed to get embedding for context building: {}", e);
                None
            }
        };

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
            warn!("Failed to build full context: {}. Using empty context.", e);
            RecallContext {
                recent: Vec::new(),
                semantic: Vec::new(),
            }
        });

        info!(
            "Context built: {} recent messages, {} semantic matches",
            context.recent.len(),
            context.semantic.len()
        );

        Ok(context)
    }

    pub async fn build_context_with_fallbacks(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        if let Ok(context) = self.build_context(session_id, user_text).await {
            return Ok(context);
        }

        warn!("Full context building failed, trying recent-only fallback");

        match self.sqlite_store
            .load_recent(session_id, self.config.history_message_cap())
            .await
        {
            Ok(recent) => {
                info!("Fallback context: {} recent messages only", recent.len());
                Ok(RecallContext {
                    recent,
                    semantic: Vec::new(),
                })
            }
            Err(e) => {
                warn!("Even recent-only context failed: {}", e);
                Ok(RecallContext {
                    recent: Vec::new(),
                    semantic: Vec::new(),
                })
            }
        }
    }

    pub async fn build_minimal_context(&self, session_id: &str) -> RecallContext {
        let recent = self.sqlite_store
            .load_recent(session_id, 5)
            .await
            .unwrap_or_else(|_| Vec::new());

        RecallContext {
            recent,
            semantic: Vec::new(),
        }
    }

    pub fn can_use_vector_search(&self) -> bool {
        self.config.enable_vector_search() && self.config.max_vector_search_results() > 0
    }

    pub async fn get_context_stats(&self, session_id: &str) -> Result<ContextStats> {
        let total_messages = self.sqlite_store
            .load_recent(session_id, usize::MAX)
            .await
            .into_api_error("Failed to load messages for stats")?
            .len();

        Ok(ContextStats {
            total_messages,
            recent_messages: self.config.history_message_cap(),
            semantic_matches: self.config.max_vector_search_results(),
        })
    }
}
