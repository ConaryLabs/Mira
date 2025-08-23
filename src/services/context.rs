// src/services/context.rs
// CONSOLIDATED: Eliminated duplication and environment variable dependency
// HIGH PRIORITY: Refactored to use ContextBuilder and centralized CONFIG

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, warn};

use crate::llm::client::OpenAIClient;
use crate::memory::recall::RecallContext;
use crate::memory::traits::MemoryStore;
use crate::services::chat::context::ContextBuilder;
use crate::services::chat::config::ChatConfig;
use crate::config::CONFIG;

/// CONSOLIDATED: Context service that eliminates duplication and uses centralized CONFIG
/// FIXED: No longer reads MIRA_CONTEXT_RECENT_MESSAGES or MIRA_CONTEXT_SEMANTIC_MATCHES
/// FIXED: Uses structured logging instead of eprintln!
#[derive(Clone)]
pub struct ContextService {
    context_builder: ContextBuilder,
}

impl ContextService {
    /// Create new ContextService using the superior ContextBuilder
    /// CONSOLIDATED: Uses centralized CONFIG instead of environment variables
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
    ) -> Self {
        // FIXED: Create configuration from centralized CONFIG (no more env vars)
        let config = ChatConfig::from_config_with_overrides(
            None, // Use default model from CONFIG
            Some(CONFIG.context_recent_messages),
            Some(CONFIG.enable_vector_search),
        );

        info!(
            "ContextService initialized with CONFIG: recent_messages={}, semantic_matches={}, vector_search={}",
            CONFIG.context_recent_messages,
            CONFIG.context_semantic_matches,
            CONFIG.enable_vector_search
        );

        // CONSOLIDATED: Use the superior ContextBuilder implementation
        let context_builder = ContextBuilder::new(
            llm_client,
            sqlite_store,
            qdrant_store,
            config,
        );

        Self { context_builder }
    }

    /// Build context for a chat session
    /// CONSOLIDATED: Delegates to ContextBuilder with structured logging (no more eprintln!)
    pub async fn build_context(
        &self,
        session_id: &str,
        embedding: Option<&[f32]>,
        _project_id: Option<&str>, // Preserved for API compatibility
    ) -> Result<RecallContext> {
        debug!("Building context for session {} using centralized CONFIG", session_id);

        // FIXED: Use centralized CONFIG values instead of environment variables
        if let Some(embedding) = embedding {
            // Use the raw build_context function for direct embedding support
            let context = crate::memory::recall::build_context(
                session_id,
                Some(embedding),
                CONFIG.context_recent_messages,  // FIXED: Use CONFIG
                CONFIG.context_semantic_matches, // FIXED: Use CONFIG  
                self.context_builder.sqlite_store.as_ref(),
                self.context_builder.qdrant_store.as_ref(),
            )
            .await
            .unwrap_or_else(|e| {
                // FIXED: Use structured logging instead of eprintln!
                warn!("Failed to build recall context: {}", e);
                RecallContext::new(vec![], vec![])
            });

            // FIXED: Structured logging with tracing instead of eprintln!
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
            // CONSOLIDATED: Use ContextBuilder for cases without direct embeddings
            let context = self.context_builder
                .build_minimal_context(session_id)
                .await;

            info!("Built minimal context: {} recent messages", context.recent.len());
            Ok(context)
        }
    }

    /// Build context with text input (recommended approach)
    /// CONSOLIDATED: Provides access to the full ContextBuilder functionality
    pub async fn build_context_with_text(
        &self,
        session_id: &str,
        user_text: &str,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        debug!("Building context with text for session {} using ContextBuilder", session_id);

        // CONSOLIDATED: Use the superior ContextBuilder with fallback strategies
        self.context_builder
            .build_context_with_fallbacks(session_id, user_text)
            .await
    }

    /// Get context statistics for debugging
    /// CONSOLIDATED: Exposes ContextBuilder's advanced features
    pub async fn get_context_stats(&self, session_id: &str) -> Result<crate::services::chat::context::ContextStats> {
        self.context_builder.get_context_stats(session_id).await
    }

    /// Check if vector search is available
    /// CONSOLIDATED: Exposes ContextBuilder's capabilities
    pub fn can_use_vector_search(&self) -> bool {
        self.context_builder.can_use_vector_search()
    }

    /// Get configuration for debugging
    /// CONSOLIDATED: Access to the unified configuration
    pub fn config(&self) -> &ChatConfig {
        self.context_builder.config()
    }
}
