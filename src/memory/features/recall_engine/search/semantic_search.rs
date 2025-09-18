// src/memory/features/recall_engine/search/semantic_search.rs

//! Semantic search - focused on vector similarity queries only.
//! 
//! Single responsibility: perform vector-based similarity search using embeddings.

use std::sync::Arc;
use anyhow::Result;
use tracing::debug;

use crate::llm::client::OpenAIClient;
use crate::memory::{
    core::types::MemoryEntry,
    storage::qdrant::multi_store::QdrantMultiStore,
};
use super::super::{ScoredMemory, RecallConfig};

#[derive(Clone)]
pub struct SemanticSearch {
    llm_client: Arc<OpenAIClient>,
    multi_store: Arc<QdrantMultiStore>,
}

impl SemanticSearch {
    pub fn new(llm_client: Arc<OpenAIClient>, multi_store: Arc<QdrantMultiStore>) -> Self {
        Self {
            llm_client,
            multi_store,
        }
    }

    /// Get embedding for a query (helper for other search strategies)
    pub async fn get_embedding(&self, query: &str) -> Result<Vec<f32>> {
        self.llm_client.get_embedding(query).await
    }

    /// Get raw search results without scoring (helper for hybrid search)
    pub async fn get_raw_results(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let embedding = self.llm_client.get_embedding(query).await?;
        
        let results_with_heads = self.multi_store
            .search_all(session_id, &embedding, limit)
            .await?;
        
        // Flatten results from all heads
        Ok(results_with_heads
            .into_iter()
            .flat_map(|(_, entries)| entries)
            .collect())
    }

    /// Semantic search using vector embeddings - clean, focused implementation
    pub async fn search(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ScoredMemory>> {
        debug!("SemanticSearch: Searching for '{}' in session {}", query, session_id);
        
        // Generate query embedding
        let embedding = self.llm_client.get_embedding(query).await?;
        
        // Search across all embedding heads (multi-head vector search)
        let results_with_heads = self.multi_store
            .search_all(session_id, &embedding, limit)
            .await?;
        
        // Flatten results from all heads
        let results: Vec<MemoryEntry> = results_with_heads
            .into_iter()
            .flat_map(|(_, entries)| entries)
            .collect();
        
        // Score results with similarity-focused weighting
        let config = RecallConfig {
            similarity_weight: 0.7,  // Emphasize similarity for semantic search
            recency_weight: 0.2,
            salience_weight: 0.1,
            ..Default::default()
        };
        
        let scored = self.score_semantic_results(results, &embedding, &config);
        
        // Limit results
        let mut final_results = scored;
        final_results.truncate(limit);
        
        debug!("SemanticSearch: Found {} semantic matches", final_results.len());
        Ok(final_results)
    }
    
    /// Score semantic search results (extracted from original scoring logic)
    fn score_semantic_results(
        &self,
        entries: Vec<MemoryEntry>,
        query_embedding: &[f32],
        config: &RecallConfig,
    ) -> Vec<ScoredMemory> {
        let mut scored: Vec<ScoredMemory> = entries
            .into_iter()
            .map(|entry| {
                let similarity_score = self.calculate_similarity_score(&entry, query_embedding);
                let salience_score = entry.salience.unwrap_or(0.0);
                
                // For semantic search, we weight similarity heavily
                let combined_score = 
                    config.similarity_weight * similarity_score +
                    config.salience_weight * salience_score;
                
                ScoredMemory {
                    entry,
                    score: combined_score,
                    recency_score: 0.0, // Not calculated in semantic-only search
                    similarity_score,
                    salience_score,
                }
            })
            .collect();
        
        // Sort by combined score (highest first)
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        
        scored
    }
    
    /// Calculate similarity score using cosine similarity (same algorithm as before)
    fn calculate_similarity_score(&self, entry: &MemoryEntry, query_embedding: &[f32]) -> f32 {
        if let Some(ref entry_embedding) = entry.embedding {
            self.cosine_similarity(entry_embedding, query_embedding)
        } else {
            0.0
        }
    }

    /// Compute cosine similarity between two vectors (same implementation as before)
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }
        
        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot_product / (norm_a * norm_b)
        }
    }
}
