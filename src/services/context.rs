// src/services/context.rs
// CONSOLIDATED: Removed duplication and migrated to use ContextBuilder with CONFIG
// Replaces environment variable reading with centralized configuration

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug};

use crate::llm::client::OpenAIClient;
use crate::memory::recall::RecallContext;
use crate::memory::traits::MemoryStore;
use crate::services::chat::context::ContextBuilder;
use crate::services::chat::config::ChatConfig;
use crate::config::CONFIG;

/// Consolidated context service using ContextBuilder and centralized CONFIG
/// MIGRATED: Removed environment variable dependencies and duplication
#[derive(Clone)]
pub struct ContextService {
    context_builder: ContextBuilder,
}

impl ContextService {
    /// Create new ContextService using the superior ContextBuilder
    /// CONSOLIDATED: Uses centralized configuration instead of environment variables
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
    ) -> Self {
        // Create configuration from centralized CONFIG
        let config = ChatConfig::from_config_with_overrides(
            None, // Use default model from CONFIG
            Some(CONFIG.context_recent_messages), // Use CONFIG values
            Some(CONFIG.enable_vector_search),
        );

        info!(
            "🔧 ContextService initialized with CONFIG values: recent_messages={}, semantic_matches={}, vector_search={}",
            CONFIG.context_recent_messages,
            CONFIG.context_semantic_matches,
            CONFIG.enable_vector_search
        );

        // Use the superior ContextBuilder implementation
        let context_builder = ContextBuilder::new(
            llm_client,
            sqlite_store,
            qdrant_store,
            config,
        );

        Self { context_builder }
    }

    /// Build context for a chat session
    /// MIGRATED: Now delegates to ContextBuilder with structured logging
    pub async fn build_context(
        &self,
        session_id: &str,
        embedding: Option<&[f32]>,
        _project_id: Option<&str>, // Preserved for API compatibility
    ) -> Result<RecallContext> {
        debug!(
            "🔍 Building context for session {} using centralized CONFIG",
            session_id
        );

        // If embedding is provided, we can't directly use it with ContextBuilder
        // which expects text. For now, we'll use the fallback approach.
        if embedding.is_some() {
            // Use the raw build_context function for direct embedding support
            let context = crate::memory::recall::build_context(
                session_id,
                embedding,
                CONFIG.context_recent_messages,
                CONFIG.context_semantic_matches,
                self.context_builder.sqlite_store.as_ref(),
                self.context_builder.qdrant_store.as_ref(),
            )
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to build recall context: {}", e);
                RecallContext::new(vec![], vec![])
            });

            // Enhanced logging with structured tracing instead of eprintln!
            if !context.semantic.is_empty() {
                info!(
                    "🔍 Built context: {} recent, {} semantic matches",
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
                info!(
                    "🔍 Built context: {} recent messages only",
                    context.recent.len()
                );
            }

            Ok(context)
        } else {
            // For cases without direct embeddings, we need some text to work with
            // This maintains backward compatibility while using the superior ContextBuilder
            let context = self.context_builder
                .build_minimal_context(session_id)
                .await;

            info!(
                "🔍 Built minimal context: {} recent messages",
                context.recent.len()
            );

            Ok(context)
        }
    }

    /// Build context with text input (recommended approach)
    /// NEW: Provides access to the full ContextBuilder functionality
    pub async fn build_context_with_text(
        &self,
        session_id: &str,
        user_text: &str,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        debug!(
            "🔍 Building context with text for session {} using ContextBuilder",
            session_id
        );

        // Use the superior ContextBuilder with fallback strategies
        self.context_builder
            .build_context_with_fallbacks(session_id, user_text)
            .await
    }

    /// Get context statistics for debugging
    /// NEW: Exposes ContextBuilder's advanced features
    pub async fn get_context_stats(&self, session_id: &str) -> Result<crate::services::chat::context::ContextStats> {
        self.context_builder.get_context_stats(session_id).await
    }

    /// Check if vector search is available
    /// NEW: Exposes ContextBuilder's capabilities
    pub fn can_use_vector_search(&self) -> bool {
        self.context_builder.can_use_vector_search()
    }

    /// Get configuration for debugging
    /// NEW: Access to the consolidated configuration
    pub fn config(&self) -> &ChatConfig {
        self.context_builder.config()
    }
}

// REMOVED: All environment variable reading
// REMOVED: All eprintln! calls
// REMOVED: Duplication of context building logic
// ADDED: Proper structured logging with tracing
// ADDED: Access to ContextBuilder's advanced features
// ADDED: Centralized CONFIG usage
