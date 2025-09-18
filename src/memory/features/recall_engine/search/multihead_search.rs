// src/memory/features/recall_engine/search/multihead_search.rs

//! Multi-head search - vector search across specific embedding heads.
//! 
//! Single responsibility: search across specified embedding heads with deduplication.

use std::sync::Arc;
use anyhow::Result;
use tracing::debug;
use chrono::Utc;

use crate::llm::{client::OpenAIClient, embeddings::EmbeddingHead};
use crate::memory::{
    core::types::MemoryEntry,
    storage::qdrant::multi_store::QdrantMultiStore,
};
use super::super::{ScoredMemory, RecallConfig};
use super::super::scoring::CompositeScorer;

#[derive(Clone)]
pub struct MultiHeadSearch {
    llm_client: Arc<OpenAIClient>,
    multi_store: Arc<QdrantMultiStore>,
    scorer: CompositeScorer,
}

impl MultiHeadSearch {
    pub fn new(llm_client: Arc<OpenAIClient>, multi_store: Arc<QdrantMultiStore>) -> Self {
        Self {
            llm_client,
            multi_store,
            scorer: CompositeScorer::new(),
        }
    }

    /// Multi-head search across specific embedding heads - same logic as original
    pub async fn search(
        &self,
        session_id: &str,
        query: &str,
        heads: &[EmbeddingHead],
        limit: usize,
    ) -> Result<Vec<ScoredMemory>> {
        debug!("MultiHeadSearch: Searching across {} heads for '{}'", heads.len(), query);
        
        // Generate query embedding
        let embedding = self.llm_client.get_embedding(query).await?;
        
        // Calculate per-head limit
        let k_per_head = limit / heads.len().max(1);
        
        // Search all heads (this returns results from all available heads, not just specified ones)
        // TODO: In the future, we might want to add head filtering to QdrantMultiStore
        let results_with_heads = self.multi_store
            .search_all(session_id, &embedding, k_per_head)
            .await?;
        
        // Flatten results from all heads
        let all_results: Vec<MemoryEntry> = results_with_heads
            .into_iter()
            .flat_map(|(_, entries)| entries)
            .collect();
        
        // Score and rank using default balanced config
        let config = RecallConfig::default();
        let mut scored = self.scorer.score_entries(
            all_results,
            &embedding,
            Utc::now(),
            &config,
        );
        
        // Deduplicate by ID (same logic as original)
        let mut seen_ids = std::collections::HashSet::new();
        scored.retain(|s| {
            if let Some(id) = s.entry.id {
                seen_ids.insert(id)
            } else {
                true
            }
        });
        
        // Limit results
        scored.truncate(limit);
        
        debug!("MultiHeadSearch: Found {} deduplicated results", scored.len());
        Ok(scored)
    }
}
