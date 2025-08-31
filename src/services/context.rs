// src/services/context.rs - Phase 5: Enhanced with MemoryService integration

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, warn};

use crate::llm::client::OpenAIClient;
use crate::memory::recall::RecallContext;
use crate::memory::qdrant::multi_store::QdrantMultiStore;
use crate::memory::traits::MemoryStore;
use crate::services::chat::context::{ContextBuilder, ContextStats};
use crate::services::chat::config::ChatConfig;
use crate::services::memory::MemoryService;
use crate::config::CONFIG;

#[derive(Clone)]
pub struct ContextService {
    context_builder: ContextBuilder,
    memory_service: Option<Arc<MemoryService>>,
}

impl ContextService {
    /// Legacy constructor for backward compatibility
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
            "ContextService initialized (legacy mode) with CONFIG: recent_messages={}, semantic_matches={}, vector_search={}",
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

        Self { 
            context_builder,
            memory_service: None,
        }
    }

    /// ── Phase 5: Enhanced constructor with MemoryService integration ──
    pub fn new_with_memory_service(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
        multi_store: Option<Arc<QdrantMultiStore>>,
        memory_service: Arc<MemoryService>,
    ) -> Self {
        let config = ChatConfig::from_config_with_overrides(
            None,
            Some(CONFIG.context_recent_messages),
            Some(CONFIG.enable_vector_search),
        );

        info!(
            "ContextService initialized (enhanced mode) with CONFIG: recent_messages={}, semantic_matches={}, vector_search={}, multi_head={}",
            CONFIG.context_recent_messages,
            CONFIG.context_semantic_matches,
            CONFIG.enable_vector_search,
            CONFIG.is_robust_memory_enabled() && multi_store.is_some()
        );

        let context_builder = ContextBuilder::new_with_multi_store(
            llm_client,
            sqlite_store,
            qdrant_store,
            multi_store,
            config,
        );

        Self { 
            context_builder,
            memory_service: Some(memory_service),
        }
    }

    /// ── Phase 5: Enhanced context building with parallel recall integration ──
    /// This method chooses the best available context building strategy.
    pub async fn build_context(
        &self,
        session_id: &str,
        embedding: Option<&[f32]>,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        debug!("Building context for session {} (enhanced_mode={})", 
               session_id, self.memory_service.is_some());

        // Phase 5: Use MemoryService parallel recall if available
        if let Some(memory_service) = &self.memory_service {
            // Check if we can use enhanced multi-head retrieval
            if CONFIG.is_robust_memory_enabled() && memory_service.is_multi_head_enabled() {
                info!("Using enhanced multi-head parallel recall for session: {}", session_id);
                return self.build_context_with_parallel_recall(session_id, embedding).await;
            } else {
                // Use MemoryService for regular parallel recall (still better than sequential)
                if let Some(_embedding) = embedding {
                    let query_text = "query"; // Placeholder - in real usage this would be the actual query
                    return memory_service.parallel_recall_context(
                        session_id,
                        query_text,
                        CONFIG.context_recent_messages,
                        CONFIG.context_semantic_matches,
                    ).await;
                }
            }
        }

        // Fallback to legacy context building
        self.build_context_legacy(session_id, embedding).await
    }

    /// ── Phase 5: Enhanced context building with text query ──
    /// This method provides the primary interface for text-based context building.
    pub async fn build_context_with_text(
        &self,
        session_id: &str,
        user_text: &str,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        debug!("Building context with text for session {} (enhanced_mode={})", 
               session_id, self.memory_service.is_some());

        // Phase 5: Use MemoryService parallel recall with text query
        if let Some(memory_service) = &self.memory_service {
            info!("Using MemoryService parallel recall with text query for session: {}", session_id);
            return memory_service.parallel_recall_context(
                session_id,
                user_text,
                CONFIG.context_recent_messages,
                CONFIG.context_semantic_matches,
            ).await;
        }

        // Fallback to ContextBuilder for legacy mode
        debug!("Falling back to ContextBuilder for text-based context");
        self.context_builder
            .build_context_with_fallbacks(session_id, user_text)
            .await
    }

    /// ── Phase 5: Parallel recall with embedding ──
    async fn build_context_with_parallel_recall(
        &self,
        session_id: &str,
        embedding: Option<&[f32]>,
    ) -> Result<RecallContext> {
        let memory_service = self.memory_service.as_ref()
            .ok_or_else(|| anyhow::anyhow!("MemoryService not available"))?;

        if let Some(_embedding_vec) = embedding {
            // For embedding-based queries, we need to create a representative query text
            // In practice, this would come from the original user query
            let query_text = "embedding_query"; // Placeholder
            
            info!("Building enhanced context with multi-head parallel recall");
            memory_service.parallel_recall_context(
                session_id,
                query_text,
                CONFIG.context_recent_messages,
                CONFIG.context_semantic_matches,
            ).await
        } else {
            // No embedding available, use minimal context
            warn!("No embedding provided for parallel recall, using minimal context");
            let recent = memory_service.get_recent_context(session_id, CONFIG.context_recent_messages).await?;
            Ok(RecallContext::new(recent, Vec::new()))
        }
    }

    /// ── Legacy context building method ──
    async fn build_context_legacy(
        &self,
        session_id: &str,
        embedding: Option<&[f32]>,
    ) -> Result<RecallContext> {
        debug!("Using legacy context building for session {}", session_id);

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
                    "Built legacy context: {} recent, {} semantic matches",
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
                info!("Built legacy context: {} recent messages only", context.recent.len());
            }

            Ok(context)
        } else {
            let context = self.context_builder.build_minimal_context(session_id).await?;
            info!("Built minimal legacy context: {} recent messages", context.recent.len());
            Ok(context)
        }
    }

    /// ── Get context statistics ──
    pub async fn get_context_stats(&self, session_id: &str) -> Result<ContextStats> {
        // Try to get enhanced stats from MemoryService first
        if let Some(memory_service) = &self.memory_service {
            if let Ok(memory_stats) = memory_service.get_service_stats(session_id).await {
                return Ok(ContextStats {
                    total_messages: memory_stats.total_messages,
                    recent_messages: memory_stats.recent_messages,
                    semantic_matches: memory_stats.semantic_entries,
                    rolling_summaries: if CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100 { 
                        memory_stats.total_messages / 10 // Rough estimate
                    } else { 
                        0 
                    },
                    compression_ratio: if memory_stats.total_messages > 0 {
                        memory_stats.semantic_entries as f64 / memory_stats.total_messages as f64
                    } else {
                        0.0
                    },
                });
            }
        }

        // Fallback to ContextBuilder stats
        self.context_builder.get_context_stats(session_id).await
    }

    /// ── Utility methods ──
    pub fn can_use_vector_search(&self) -> bool {
        self.context_builder.can_use_vector_search()
    }

    pub fn config(&self) -> &ChatConfig {
        self.context_builder.config()
    }

    pub fn is_enhanced_mode(&self) -> bool {
        self.memory_service.is_some()
    }

    /// ── Phase 5: Build minimal context (direct fallback) ──
    pub async fn build_minimal_context(&self, session_id: &str) -> Result<RecallContext> {
        if let Some(memory_service) = &self.memory_service {
            let recent = memory_service.get_recent_context(session_id, CONFIG.context_recent_messages).await?;
            Ok(RecallContext::new(recent, Vec::new()))
        } else {
            self.context_builder.build_minimal_context(session_id).await
        }
    }

    /// ── Phase 5: Health check for context service ──
    pub async fn health_check(&self) -> Result<ContextServiceHealth> {
        let enhanced_mode = self.is_enhanced_mode();
        let vector_search_enabled = self.can_use_vector_search();
        
        let multi_head_available = if let Some(memory_service) = &self.memory_service {
            memory_service.is_multi_head_enabled()
        } else {
            false
        };

        let parallel_recall_available = self.memory_service.is_some();

        Ok(ContextServiceHealth {
            enhanced_mode,
            vector_search_enabled,
            multi_head_available,
            parallel_recall_available,
            robust_memory_enabled: CONFIG.is_robust_memory_enabled(),
            rolling_summaries_enabled: CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100,
        })
    }
}

/// ── Phase 5: Context service health status ──
#[derive(Debug, Clone)]
pub struct ContextServiceHealth {
    pub enhanced_mode: bool,
    pub vector_search_enabled: bool,
    pub multi_head_available: bool,
    pub parallel_recall_available: bool,
    pub robust_memory_enabled: bool,
    pub rolling_summaries_enabled: bool,
}
