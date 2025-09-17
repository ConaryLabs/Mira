// src/memory/features/scoring.rs
// Composite scoring system with decay, salience, and special boosts

use chrono::{DateTime, Utc};
use tracing::{debug, info};
use crate::memory::core::types::MemoryEntry;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::features::memory_types::ScoredMemoryEntry;

/// Advanced memory scoring with multiple factors
pub struct MemoryScorer {
    similarity_weight: f32,
    salience_weight: f32,
    recency_weight: f32,
    pin_boost_factor: f32,
    summary_boost_factor: f32,
    decay_half_life_hours: f32,
}

impl MemoryScorer {
    /// Creates a new scorer with default weights
    pub fn new() -> Self {
        Self {
            similarity_weight: 0.4,      // 40% weight on semantic similarity
            salience_weight: 0.3,         // 30% weight on importance
            recency_weight: 0.3,          // 30% weight on freshness
            pin_boost_factor: 2.0,        // 2x boost for pinned items
            summary_boost_factor: 1.5,    // 1.5x boost for summaries
            decay_half_life_hours: 24.0,  // Half-life of 24 hours for recency
        }
    }
    
    /// Creates a scorer with custom weights
    pub fn with_weights(
        similarity: f32,
        salience: f32,
        recency: f32,
        pin_boost: f32,
        summary_boost: f32,
    ) -> Self {
        // Normalize weights to sum to 1.0
        let total = similarity + salience + recency;
        Self {
            similarity_weight: similarity / total,
            salience_weight: salience / total,
            recency_weight: recency / total,
            pin_boost_factor: pin_boost,
            summary_boost_factor: summary_boost,
            decay_half_life_hours: 24.0,
        }
    }
    
    /// Calculates the composite score for a memory entry
    pub fn calculate_composite_score(
        &self,
        entry: &MemoryEntry,
        similarity: f32,
        now: DateTime<Utc>,
    ) -> f32 {
        // Component 1: Salience (importance) normalized to 0-1
        let salience = entry.salience.unwrap_or(5.0) / 10.0;
        
        // Component 2: Recency with exponential decay
        let recency = self.calculate_recency_score(entry, now);
        
        // Component 3: Similarity is already 0-1 from cosine similarity
        
        // Base composite score (weighted combination)
        let base_score = self.similarity_weight * similarity
            + self.salience_weight * salience
            + self.recency_weight * recency;
        
        // Apply special boosts
        let mut final_score = base_score;
        
        // Note: Pinned boost removed since 'pinned' field doesn't exist
        // Could check tags for "pinned" if needed
        if let Some(ref tags) = entry.tags {
            if tags.iter().any(|t| t.contains("pinned")) {
                final_score *= self.pin_boost_factor;
                debug!("Applied pin boost: {:.3} -> {:.3}", base_score, final_score);
            }
        }
        
        // Summary boost - summaries are gold
        if self.is_summary(entry) {
            final_score *= self.summary_boost_factor;
            debug!("Applied summary boost: {:.3} -> {:.3}", base_score, final_score);
        }
        
        final_score
    }
    
    /// Calculates recency score with exponential decay
    fn calculate_recency_score(&self, entry: &MemoryEntry, now: DateTime<Utc>) -> f32 {
        let last_access = entry.last_recalled.unwrap_or(entry.timestamp);
        let age_hours = now.signed_duration_since(last_access).num_hours() as f32;
        
        // Exponential decay: e^(-λt) where λ = ln(2)/half_life
        let lambda = 0.693 / self.decay_half_life_hours;
        (-lambda * age_hours).exp()
    }
    
    /// Checks if an entry is a summary
    fn is_summary(&self, entry: &MemoryEntry) -> bool {
        entry.tags.as_ref()
            .map(|tags| tags.iter().any(|t| t.contains("summary")))
            .unwrap_or(false)
    }
    
    /// Calculates cosine similarity between two embedding vectors
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }
        
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }
    
    /// Scores and ranks multiple memory entries
    pub fn score_entries(
        &self,
        entries: Vec<(EmbeddingHead, Vec<MemoryEntry>)>,
        query_embedding: &[f32],
        now: DateTime<Utc>,
    ) -> Vec<ScoredMemoryEntry> {
        let mut scored_entries = Vec::new();
        
        for (head, head_entries) in entries {
            debug!("Scoring {} entries from {} head", head_entries.len(), head.as_str());
            
            for entry in head_entries {
                let similarity = if let Some(ref entry_embedding) = entry.embedding {
                    Self::cosine_similarity(query_embedding, entry_embedding)
                } else {
                    0.0
                };
                
                let composite = self.calculate_composite_score(&entry, similarity, now);
                
                // Calculate individual component scores for debugging
                let salience_score = entry.salience.unwrap_or(5.0) / 10.0;
                let recency_score = self.calculate_recency_score(&entry, now);
                
                scored_entries.push(ScoredMemoryEntry {
                    entry,
                    similarity_score: similarity,
                    salience_score,
                    recency_score,
                    composite_score: composite,
                    source_head: head,
                });
            }
        }
        
        // Sort by composite score (highest first)
        scored_entries.sort_by(|a, b| {
            b.composite_score.partial_cmp(&a.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        
        if !scored_entries.is_empty() {
            info!(
                "Scored {} entries - Top score: {:.3}, Bottom score: {:.3}",
                scored_entries.len(),
                scored_entries.first().map(|e| e.composite_score).unwrap_or(0.0),
                scored_entries.last().map(|e| e.composite_score).unwrap_or(0.0)
            );
        }
        
        scored_entries
    }
    
    /// Filters entries based on minimum score threshold
    pub fn filter_by_threshold(
        &self,
        entries: Vec<ScoredMemoryEntry>,
        min_score: f32,
    ) -> Vec<ScoredMemoryEntry> {
        let original_count = entries.len();
        let filtered: Vec<_> = entries.into_iter()
            .filter(|e| e.composite_score >= min_score)
            .collect();
        
        if filtered.len() < original_count {
            debug!(
                "Filtered {} entries below threshold {:.3}",
                original_count - filtered.len(),
                min_score
            );
        }
        
        filtered
    }
    
    /// Gets the top K entries
    pub fn top_k(entries: Vec<ScoredMemoryEntry>, k: usize) -> Vec<ScoredMemoryEntry> {
        entries.into_iter().take(k).collect()
    }
    
    /// Analyzes score distribution for monitoring
    pub fn analyze_distribution(entries: &[ScoredMemoryEntry]) -> ScoreDistribution {
        if entries.is_empty() {
            return ScoreDistribution::default();
        }
        
        let scores: Vec<f32> = entries.iter().map(|e| e.composite_score).collect();
        let sum: f32 = scores.iter().sum();
        let mean = sum / scores.len() as f32;
        
        let variance = scores.iter()
            .map(|s| (s - mean).powi(2))
            .sum::<f32>() / scores.len() as f32;
        let std_dev = variance.sqrt();
        
        ScoreDistribution {
            count: entries.len(),
            mean,
            std_dev,
            min: scores.iter().cloned().fold(f32::INFINITY, f32::min),
            max: scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max),
            median: {
                let mut sorted = scores.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                sorted[sorted.len() / 2]
            },
        }
    }
}

/// Statistics about score distribution
#[derive(Debug, Default)]
pub struct ScoreDistribution {
    pub count: usize,
    pub mean: f32,
    pub std_dev: f32,
    pub min: f32,
    pub max: f32,
    pub median: f32,
}

impl Default for MemoryScorer {
    fn default() -> Self {
        Self::new()
    }
}
