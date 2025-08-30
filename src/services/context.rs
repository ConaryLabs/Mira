// src/services/context.rs - Fixed version

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, warn};

use crate::llm::client::OpenAIClient;
use crate::memory::recall::RecallContext;
use crate::memory::traits::MemoryStore;
use crate::services::chat::context::ContextBuilder;
use crate::services::chat::config::ChatConfig;
use crate::config::CONFIG;

#[derive(Clone)]
pub struct ContextService {
    context_builder: ContextBuilder,
}

impl ContextService {
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
    ) -> Self {
        let config = ChatConfig::from_config_with_overrides(
            None,
            Some(CONFIG.context_recent_messages),
            Some(CONFIG.enable_vector_search),
        );

        info!(
            "ContextService initialized with CONFIG: recent_messages={}, semantic_matches={}, vector_search={}",
            CONFIG.context_recent_messages,
            CONFIG.context_semantic_matches,
            CONFIG.enable_vector_search
        );

        let context_builder = ContextBuilder::new(
            llm_client,
            sqlite_store,
            qdrant_store,
            config,
        );

        Self { context_builder }
    }

    pub async fn build_context(
        &self,
        session_id: &str,
        embedding: Option<&[f32]>,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        debug!("Building context for session {} using centralized CONFIG", session_id);

        if let Some(embedding) = embedding {
            let context = crate::memory::recall::build_context(
                session_id,
                Some(embedding),
                CONFIG.context_recent_messages,
                CONFIG.context_semantic_matches,
                self.context_builder.sqlite_store().as_ref(),
                self.context_builder.qdrant_store().as_ref(),
            )
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to build recall context: {}", e);
                RecallContext::new(vec![], vec![])
            });

            if !context.semantic.is_empty() {
                info!(
                    "Built context: {} recent, {} semantic matches",
                    context.recent.len(),
                    context.semantic.len()
                );

                debug!("Top semantic matches:");
                for (i, msg) in context.semantic.iter().take(3).enumerate() {
                    debug!(
                        "  {}. [salience: {:.2}] {}",
                        i + 1,
                        msg.salience.unwrap_or(0.0),
                        msg.content.chars().take(60).collect::<String>()
                    );
                }
            } else {
                info!("Built context: {} recent messages only", context.recent.len());
            }

            Ok(context)
        } else {
            let context = self.context_builder.build_minimal_context(session_id).await?;
            info!("Built minimal context: {} recent messages", context.recent.len());
            Ok(context)
        }
    }

    pub async fn build_context_with_text(
        &self,
        session_id: &str,
        user_text: &str,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        debug!("Building context with text for session {} using ContextBuilder", session_id);

        self.context_builder
            .build_context_with_fallbacks(session_id, user_text)
            .await
    }

    pub async fn get_context_stats(&self, session_id: &str) -> Result<crate::services::chat::context::ContextStats> {
        self.context_builder.get_context_stats(session_id).await
    }

    pub fn can_use_vector_search(&self) -> bool {
        self.context_builder.can_use_vector_search()
    }

    pub fn config(&self) -> &ChatConfig {
        self.context_builder.config()
    }
}
