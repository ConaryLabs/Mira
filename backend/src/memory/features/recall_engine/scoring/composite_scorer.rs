// src/memory/features/recall_engine/scoring/composite_scorer.rs

//! Composite scoring - unified scoring algorithms for all search types.
//!
//! Single responsibility: calculate relevance scores using multiple factors.

use chrono::{DateTime, Utc};

use super::super::{RecallConfig, ScoredMemory};
use crate::memory::core::types::MemoryEntry;

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
                let project_score =
                    self.calculate_project_score(&entry, config.current_project_id.as_deref());

                // Weighted composite score
                let combined_score = config.recency_weight * recency_score
                    + config.similarity_weight * similarity_score
                    + config.salience_weight * salience_score
                    + config.project_weight * project_score;

                ScoredMemory {
                    entry,
                    score: combined_score,
                    recency_score,
                    similarity_score,
                    salience_score,
                    project_score,
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

    /// Calculate project match score
    ///
    /// Returns 1.0 if entry's project matches current project (or both are None),
    /// 0.3 otherwise (mismatch allows cross-project context with lower weight)
    pub fn calculate_project_score(
        &self,
        entry: &MemoryEntry,
        current_project_id: Option<&str>,
    ) -> f32 {
        let entry_project = entry.extract_project_from_tags();

        match (entry_project.as_deref(), current_project_id) {
            // Both have same project - full match
            (Some(entry_proj), Some(current)) if entry_proj == current => 1.0,
            // Both have no project - considered a match
            (None, None) => 1.0,
            // Mismatch or unknown - lower score but not zero (cross-project context allowed)
            _ => 0.3,
        }
    }
}
