// src/memory/features/recall_engine/context/memory_builder.rs
//! Memory context builder - assembles recall context from search results.
//! 
//! Single responsibility: build RecallContext using hybrid search results.

use anyhow::Result;
use tracing::info;
use crate::memory::core::types::MemoryEntry;
use super::super::{RecallContext, RecallConfig};
use super::super::search::HybridSearch;

#[derive(Clone)]
pub struct MemoryContextBuilder {
    hybrid_search: HybridSearch,
}

impl MemoryContextBuilder {
    pub fn new(hybrid_search: HybridSearch) -> Self {
        Self {
            hybrid_search,
        }
    }
    
    /// Build recall context - same logic as original build_recall_context
    pub async fn build_context(
        &self,
        session_id: &str,
        query: &str,
        config: RecallConfig,
    ) -> Result<RecallContext> {
        info!("MemoryContextBuilder: Building context for session {}", session_id);
        
        // Use hybrid search to get combined recent + semantic results
        let scored_results = self.hybrid_search
            .search(session_id, query, &config)
            .await?;
        
        // Split results back into recent and semantic for backward compatibility
        // This maintains the original RecallContext structure that existing code expects
        let (recent, semantic) = self.split_results_for_context(scored_results, &config);
        
        info!("MemoryContextBuilder: Built context - {} recent, {} semantic", 
              recent.len(), semantic.len());
        
        // PHASE 1.1 FIX: Add summary fields (initialized as None - will be populated by unified_handler)
        Ok(RecallContext { 
            recent, 
            semantic,
            rolling_summary: None,
            session_summary: None,
        })
    }
    
    /// Split hybrid results back into recent/semantic categories for RecallContext
    /// 
    /// This is needed to maintain backward compatibility with the original RecallContext
    /// structure that separates recent and semantic results.
    fn split_results_for_context(
        &self,
        scored_results: Vec<super::super::ScoredMemory>,
        config: &RecallConfig,
    ) -> (Vec<MemoryEntry>, Vec<MemoryEntry>) {
        // Take top entries and split based on recency vs similarity scores
        let mut recent = Vec::new();
        let mut semantic = Vec::new();
        
        for scored in scored_results {
            // If recency score is higher than similarity score, classify as "recent"
            // Otherwise classify as "semantic"
            if scored.recency_score > scored.similarity_score && recent.len() < config.recent_count {
                recent.push(scored.entry);
            } else if semantic.len() < config.semantic_count {
                semantic.push(scored.entry);
            }
            
            // Stop when we have enough of both types
            if recent.len() >= config.recent_count && semantic.len() >= config.semantic_count {
                break;
            }
        }
        
        (recent, semantic)
    }
}
