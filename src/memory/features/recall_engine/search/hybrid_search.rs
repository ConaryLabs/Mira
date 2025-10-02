// src/memory/features/recall_engine/search/hybrid_search.rs
//! Hybrid search - combines recent and semantic search strategies.
//! 
//! Single responsibility: orchestrate parallel recent + semantic search with deduplication.

use anyhow::Result;
use tracing::debug;
use chrono::Utc;
use super::{RecentSearch, SemanticSearch};
use super::super::{ScoredMemory, RecallConfig};
use super::super::scoring::CompositeScorer;

#[derive(Clone)]
pub struct HybridSearch {
    recent_search: RecentSearch,
    semantic_search: SemanticSearch,
    scorer: CompositeScorer,
}

impl HybridSearch {
    pub fn new(recent_search: RecentSearch, semantic_search: SemanticSearch) -> Self {
        Self {
            recent_search,
            semantic_search,
            scorer: CompositeScorer::new(),
        }
    }
    
    /// Hybrid search combining recent and semantic - same logic as original, cleaner implementation
    pub async fn search(
        &self,
        session_id: &str,
        query: &str,
        config: &RecallConfig,
    ) -> Result<Vec<ScoredMemory>> {
        debug!("HybridSearch: Combining recent + semantic for '{}' in session {}", query, session_id);
        
        // Parallel retrieval of recent scored results and raw semantic results
        let (recent_scored, semantic_raw) = tokio::try_join!(
            self.recent_search.search(session_id, config.recent_count),
            self.semantic_search.get_raw_results(session_id, query, config.k_per_head * 3)
        )?;
        
        // Score semantic results with full composite scoring (including recency)
        let query_embedding = self.semantic_search.get_embedding(query).await?;
        let semantic_scored = self.scorer.score_entries(
            semantic_raw,
            &query_embedding,
            Utc::now(),
            config,
        );
        
        // Combine and deduplicate results
        let combined = self.combine_and_deduplicate(recent_scored, semantic_scored, config);
        
        debug!("HybridSearch: Combined results: {} entries", combined.len());
        Ok(combined)
    }
    
    /// Combine and deduplicate scored results (same logic as original)
    fn combine_and_deduplicate(
        &self,
        mut recent: Vec<ScoredMemory>,
        mut semantic: Vec<ScoredMemory>,
        config: &RecallConfig,
    ) -> Vec<ScoredMemory> {
        let mut combined = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        
        // Add recent entries first (they get priority in deduplication)
        for scored in recent.drain(..) {
            if let Some(id) = scored.entry.id {
                seen_ids.insert(id);
                combined.push(scored);
            }
        }
        
        // Add semantic entries (avoiding duplicates)
        for scored in semantic.drain(..) {
            if let Some(id) = scored.entry.id {
                if !seen_ids.contains(&id) {
                    combined.push(scored);
                }
            }
        }
        
        // Re-sort by composite score
        combined.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        
        // Limit to requested size
        combined.truncate(config.recent_count + config.semantic_count);
        
        combined
    }
}
