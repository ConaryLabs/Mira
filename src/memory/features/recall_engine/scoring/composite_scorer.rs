// src/memory/features/recall_engine/scoring/composite_scorer.rs

//! Composite scoring - unified scoring algorithms for all search types.
//! 
//! Single responsibility: calculate relevance scores using multiple factors.

use chrono::{DateTime, Utc};

use crate::memory::core::types::MemoryEntry;
use super::super::{ScoredMemory, RecallConfig};

#[derive(Clone)]
pub struct CompositeScorer;

impl CompositeScorer {
    pub fn new() -> Self {
        Self
    }

    /// Score entries using multiple factors - same algorithm as before, cleaner implementation
    pub fn score_entries(
        &self,
        entries: Vec<MemoryEntry>,
        query_embedding: &[f32],
        now: DateTime<Utc>,
        config: &RecallConfig,
    ) -> Vec<ScoredMemory> {
        let mut scored: Vec<ScoredMemory> = entries
            .into_iter()
            .map(|entry| {
                // Calculate individual scores
                let recency_score = self.calculate_recency_score(&entry, now);
                let similarity_score = self.calculate_similarity_score(&entry, query_embedding);
                let salience_score = entry.salience.unwrap_or(0.0);
                
                // Weighted composite score
                let combined_score = 
                    config.recency_weight * recency_score +
                    config.similarity_weight * similarity_score +
                    config.salience_weight * salience_score;
                
                ScoredMemory {
                    entry,
                    score: combined_score,
                    recency_score,
                    similarity_score,
                    salience_score,
                }
            })
            .collect();
        
        // Sort by combined score (highest first)
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        
        scored
    }

    /// Calculate recency score with exponential decay (same algorithm as original)
    pub fn calculate_recency_score(&self, entry: &MemoryEntry, now: DateTime<Utc>) -> f32 {
        let age_hours = (now - entry.timestamp).num_hours() as f32;
        (-age_hours / 24.0).exp() // Exponential decay over days
    }

    /// Calculate similarity score using cosine similarity (same algorithm as original)
    pub fn calculate_similarity_score(&self, entry: &MemoryEntry, query_embedding: &[f32]) -> f32 {
        if let Some(ref entry_embedding) = entry.embedding {
            self.cosine_similarity(entry_embedding, query_embedding)
        } else {
            0.0
        }
    }

    /// Compute cosine similarity between two vectors (same implementation as original)
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
