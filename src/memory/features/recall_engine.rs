// src/memory/features/recall_engine.rs

//! Unified search and recall engine for memory retrieval.
//! Consolidates all search, scoring, and retrieval logic.

use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info};
use chrono::{DateTime, Utc};

use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::{
    core::types::MemoryEntry,
    core::traits::MemoryStore,  // ADDED - needed for load_recent trait method
    storage::sqlite::store::SqliteMemoryStore,
    storage::qdrant::multi_store::QdrantMultiStore,
};

// ===== DATA STRUCTURES =====

/// Context for recall operations containing recent and semantic memories
#[derive(Debug, Clone)]
pub struct RecallContext {
    pub recent: Vec<MemoryEntry>,
    pub semantic: Vec<MemoryEntry>,
}

/// Configuration for recall operations
#[derive(Debug, Clone)]
pub struct RecallConfig {
    /// Number of recent messages to retrieve
    pub recent_count: usize,
    /// Number of semantic results to retrieve
    pub semantic_count: usize,
    /// Number of results per embedding head
    pub k_per_head: usize,
    /// Weight for recency scoring (0.0 to 1.0)
    pub recency_weight: f32,
    /// Weight for semantic similarity (0.0 to 1.0)
    pub similarity_weight: f32,
    /// Weight for salience scoring (0.0 to 1.0)
    pub salience_weight: f32,
}

impl Default for RecallConfig {
    fn default() -> Self {
        Self {
            recent_count: 10,
            semantic_count: 20,
            k_per_head: 10,
            recency_weight: 0.3,
            similarity_weight: 0.5,
            salience_weight: 0.2,
        }
    }
}

/// Search request types
#[derive(Debug)]
pub enum SearchMode {
    /// Recent messages only
    Recent { limit: usize },
    /// Semantic search only
    Semantic { query: String, limit: usize },
    /// Combined recent + semantic
    Hybrid { query: String, config: RecallConfig },
    /// Multi-head semantic search
    MultiHead { query: String, heads: Vec<EmbeddingHead>, limit: usize },
}

/// Scored memory entry with relevance metrics
#[derive(Debug, Clone)]
pub struct ScoredMemory {
    pub entry: MemoryEntry,
    pub score: f32,
    pub recency_score: f32,
    pub similarity_score: f32,
    pub salience_score: f32,
}

// ===== RECALL ENGINE =====

/// Unified recall engine for all memory retrieval operations
pub struct RecallEngine {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<SqliteMemoryStore>,
    multi_store: Arc<QdrantMultiStore>,
    default_config: RecallConfig,
}

impl RecallEngine {
    /// Creates a new recall engine
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            multi_store,
            default_config: RecallConfig::default(),
        }
    }

    /// Main entry point for all search operations
    pub async fn search(
        &self,
        session_id: &str,
        mode: SearchMode,
    ) -> Result<Vec<ScoredMemory>> {
        match mode {
            SearchMode::Recent { limit } => {
                self.search_recent(session_id, limit).await
            }
            SearchMode::Semantic { query, limit } => {
                self.search_semantic(session_id, &query, limit).await
            }
            SearchMode::Hybrid { query, config } => {
                self.search_hybrid(session_id, &query, config).await
            }
            SearchMode::MultiHead { query, heads, limit } => {
                self.search_multihead(session_id, &query, &heads, limit).await
            }
        }
    }

    /// Builds a recall context (backward compatibility)
    pub async fn build_recall_context(
        &self,
        session_id: &str,
        query: &str,
        config: Option<RecallConfig>,
    ) -> Result<RecallContext> {
        let config = config.unwrap_or_default();
        
        info!("Building recall context for session: {}", session_id);
        
        // Parallel retrieval
        let (embedding_result, recent_result) = tokio::join!(
            self.llm_client.get_embedding(query),
            self.sqlite_store.load_recent(session_id, config.recent_count)
        );
        
        let embedding = embedding_result?;
        let recent = recent_result?;
        
        // Multi-head semantic search - returns Vec<(EmbeddingHead, Vec<MemoryEntry>)>
        let results_with_heads = self.multi_store
            .search_all(session_id, &embedding, config.k_per_head)
            .await?;
        
        // FIXED: Flatten the results from all heads
        let semantic_results: Vec<MemoryEntry> = results_with_heads
            .into_iter()
            .flat_map(|(_, entries)| entries)
            .collect();
        
        // Score and rank
        let now = Utc::now();
        let scored = self.score_entries(semantic_results, &embedding, now, &config);
        
        // Take top results
        let semantic: Vec<MemoryEntry> = scored
            .into_iter()
            .take(config.semantic_count)
            .map(|s| s.entry)
            .collect();
        
        Ok(RecallContext { recent, semantic })
    }

    /// Search recent messages only
    async fn search_recent(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ScoredMemory>> {
        debug!("Searching {} recent messages for session {}", limit, session_id);
        
        let entries = self.sqlite_store.load_recent(session_id, limit).await?;
        
        // Convert to scored entries with recency scores
        let now = Utc::now();
        Ok(entries.into_iter().map(|entry| {
            let recency_score = self.calculate_recency_score(&entry, now);
            ScoredMemory {
                score: recency_score, // Only recency matters here
                recency_score,
                similarity_score: 0.0,
                salience_score: entry.salience.unwrap_or(0.0),
                entry,
            }
        }).collect())
    }

    /// Semantic search only
    async fn search_semantic(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ScoredMemory>> {
        debug!("Semantic search for '{}' in session {}", query, session_id);
        
        let embedding = self.llm_client.get_embedding(query).await?;
        
        // FIXED: Handle tuple return type
        let results_with_heads = self.multi_store
            .search_all(session_id, &embedding, limit)
            .await?;
        
        let results: Vec<MemoryEntry> = results_with_heads
            .into_iter()
            .flat_map(|(_, entries)| entries)
            .collect();
        
        let now = Utc::now();
        let config = RecallConfig {
            similarity_weight: 0.7,  // Emphasize similarity for semantic search
            recency_weight: 0.2,
            salience_weight: 0.1,
            ..Default::default()
        };
        
        let mut scored = self.score_entries(results, &embedding, now, &config);
        scored.truncate(limit);
        
        Ok(scored)
    }

    /// Hybrid search combining recent and semantic
    async fn search_hybrid(
        &self,
        session_id: &str,
        query: &str,
        config: RecallConfig,
    ) -> Result<Vec<ScoredMemory>> {
        debug!("Hybrid search for '{}' in session {}", query, session_id);
        
        // Parallel retrieval
        let (embedding_result, recent_result) = tokio::join!(
            self.llm_client.get_embedding(query),
            self.search_recent(session_id, config.recent_count)
        );
        
        let embedding = embedding_result?;
        let mut recent = recent_result?;
        
        // FIXED: Handle tuple return type from search_all
        let semantic_results_with_heads = self.multi_store
            .search_all(session_id, &embedding, config.k_per_head * 3)
            .await?;
        
        let semantic_results: Vec<MemoryEntry> = semantic_results_with_heads
            .into_iter()
            .flat_map(|(_, entries)| entries)
            .collect();
        
        // Score semantic results
        let now = Utc::now();
        let mut semantic = self.score_entries(semantic_results, &embedding, now, &config);
        
        // Combine and deduplicate
        let mut combined = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        
        // Add recent entries first
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
        
        // Re-sort by score
        combined.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        combined.truncate(config.recent_count + config.semantic_count);
        
        Ok(combined)
    }

    /// Multi-head search across specific embedding heads
    async fn search_multihead(
        &self,
        session_id: &str,
        query: &str,
        heads: &[EmbeddingHead],
        limit: usize,
    ) -> Result<Vec<ScoredMemory>> {
        debug!("Multi-head search across {} heads", heads.len());
        
        let embedding = self.llm_client.get_embedding(query).await?;
        let k_per_head = limit / heads.len().max(1);
        
        // FIXED: Handle tuple return type properly
        let results_with_heads = self.multi_store
            .search_all(session_id, &embedding, k_per_head)
            .await?;
        
        let all_results: Vec<MemoryEntry> = results_with_heads
            .into_iter()
            .flat_map(|(_, entries)| entries)
            .collect();
        
        // Score and rank
        let now = Utc::now();
        let mut scored = self.score_entries(all_results, &embedding, now, &self.default_config);
        
        // Deduplicate by ID
        let mut seen_ids = std::collections::HashSet::new();
        scored.retain(|s| {
            if let Some(id) = s.entry.id {
                seen_ids.insert(id)
            } else {
                true
            }
        });
        
        scored.truncate(limit);
        Ok(scored)
    }

    /// Scores entries based on multiple factors
    fn score_entries(
        &self,
        entries: Vec<MemoryEntry>,
        query_embedding: &[f32],
        now: DateTime<Utc>,
        config: &RecallConfig,
    ) -> Vec<ScoredMemory> {
        let mut scored: Vec<ScoredMemory> = entries
            .into_iter()
            .map(|entry| {
                let recency_score = self.calculate_recency_score(&entry, now);
                let similarity_score = self.calculate_similarity_score(&entry, query_embedding);
                let salience_score = entry.salience.unwrap_or(0.0);
                
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

    /// Calculate recency score (exponential decay)
    fn calculate_recency_score(&self, entry: &MemoryEntry, now: DateTime<Utc>) -> f32 {
        let age_hours = (now - entry.timestamp).num_hours() as f32;
        (-age_hours / 24.0).exp() // Exponential decay over days
    }

    /// Calculate similarity score
    fn calculate_similarity_score(&self, entry: &MemoryEntry, query_embedding: &[f32]) -> f32 {
        if let Some(ref entry_embedding) = entry.embedding {
            self.cosine_similarity(entry_embedding, query_embedding)
        } else {
            0.0
        }
    }

    /// Compute cosine similarity between two vectors
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
