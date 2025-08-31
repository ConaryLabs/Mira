// src/services/context.rs

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug};

use crate::memory::recall::RecallContext;
use crate::services::chat::context::{ContextBuilder, ContextStats};
use crate::services::memory::MemoryService;
use crate::config::CONFIG;

#[derive(Clone)]
pub struct ContextService {
    context_builder: ContextBuilder,
    memory_service: Arc<MemoryService>,
}

impl ContextService {
    /// Constructor using the robust MemoryService.
    pub fn new(memory_service: Arc<MemoryService>) -> Self {
        info!("ContextService initialized in robust mode");

        let context_builder = ContextBuilder::new(
            memory_service.clone(),
            Default::default(), // Use default ChatConfig
        );

        Self { 
            context_builder,
            memory_service,
        }
    }

    /// This method provides the primary interface for text-based context building.
    pub async fn build_context_with_text(
        &self,
        session_id: &str,
        user_text: &str,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        debug!("Building context with text for session {}", session_id);
        
        info!("Using MemoryService parallel recall with text query for session: {}", session_id);
        self.memory_service.parallel_recall_context(
            session_id,
            user_text,
            CONFIG.context_recent_messages,
            CONFIG.context_semantic_matches,
        ).await
    }

    /// Get context statistics.
    pub async fn get_context_stats(&self, session_id: &str) -> Result<ContextStats> {
        let memory_stats = self.memory_service.get_service_stats(session_id).await?;
        Ok(ContextStats {
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
        })
    }

    /// Health check for context service.
    pub async fn health_check(&self) -> Result<ContextServiceHealth> {
        Ok(ContextServiceHealth {
            vector_search_enabled: CONFIG.enable_vector_search,
            multi_head_available: true,
            parallel_recall_available: true,
            robust_memory_enabled: CONFIG.is_robust_memory_enabled(),
            rolling_summaries_enabled: CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100,
        })
    }
}

/// Context service health status.
#[derive(Debug, Clone)]
pub struct ContextServiceHealth {
    pub vector_search_enabled: bool,
    pub multi_head_available: bool,
    pub parallel_recall_available: bool,
    pub robust_memory_enabled: bool,
    pub rolling_summaries_enabled: bool,
}
