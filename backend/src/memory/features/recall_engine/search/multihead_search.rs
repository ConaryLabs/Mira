// src/memory/features/recall_engine/search/multihead_search.rs

//! Multi-head search - vector search across specific embedding heads.
//!
//! Single responsibility: search across specified embedding heads with deduplication.

use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tracing::debug;

use super::super::scoring::CompositeScorer;
use super::super::{RecallConfig, ScoredMemory};
use crate::llm_compat::{embeddings::EmbeddingHead, provider::EmbeddingProvider};
use crate::memory::{core::types::MemoryEntry, storage::qdrant::multi_store::QdrantMultiStore};

#[derive(Clone)]
pub struct MultiHeadSearch {
    embedding_client: Arc<dyn EmbeddingProvider>,
    multi_store: Arc<QdrantMultiStore>,
    scorer: CompositeScorer,
}

impl MultiHeadSearch {
    pub fn new(
        embedding_client: Arc<dyn EmbeddingProvider>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            embedding_client,
            multi_store,
            scorer: CompositeScorer::new(),
        }
    }

    /// Multi-head search across specific embedding heads
    pub async fn search(
        &self,
        session_id: &str,
        query: &str,
        heads: &[EmbeddingHead],
        limit: usize,
    ) -> Result<Vec<ScoredMemory>> {
        debug!(
            "MultiHeadSearch: Searching across {} heads for '{}'",
            heads.len(),
            query
        );

        if heads.is_empty() {
            debug!("MultiHeadSearch: No heads specified, returning empty results");
            return Ok(vec![]);
        }

        // Generate query embedding with explicit type
        let embedding: Vec<f32> = self.embedding_client.embed(query).await?;

        // Calculate per-head limit (at least 1)
        let k_per_head = (limit / heads.len()).max(1);

        // Search only the specified heads in parallel
        let search_futures: Vec<_> = heads
            .iter()
            .map(|head| {
                let multi_store = self.multi_store.clone();
                let session_id = session_id.to_string();
                let embedding = embedding.clone();
                let head = *head;
                async move {
                    let result = multi_store
                        .search(head, &session_id, &embedding, k_per_head)
                        .await;
                    (head, result)
                }
            })
            .collect();

        let results = futures::future::join_all(search_futures).await;

        // Flatten results from searched heads only
        let mut all_results: Vec<MemoryEntry> = Vec::new();
        for (head, result) in results {
            match result {
                Ok(entries) => {
                    debug!(
                        "MultiHeadSearch: Found {} results from {:?} head",
                        entries.len(),
                        head
                    );
                    all_results.extend(entries);
                }
                Err(e) => {
                    debug!("MultiHeadSearch: Error searching {:?} head: {}", head, e);
                }
            }
        }

        // Score and rank using default balanced config
        let config = RecallConfig::default();
        let mut scored = self
            .scorer
            .score_entries(all_results, &embedding, Utc::now(), &config);

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

        debug!(
            "MultiHeadSearch: Found {} deduplicated results from {} heads",
            scored.len(),
            heads.len()
        );
        Ok(scored)
    }
}
