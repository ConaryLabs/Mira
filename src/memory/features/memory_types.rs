// src/services/memory/types.rs
// Shared types and structs for the memory service modules

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::core::types::{MemoryEntry, MemoryType};

/// Scored memory entry with all scoring components
#[derive(Debug, Clone)]
pub struct ScoredMemoryEntry {
    pub entry: MemoryEntry,
    pub similarity_score: f32,
    pub salience_score: f32,
    pub recency_score: f32,
    pub composite_score: f32,
    pub source_head: EmbeddingHead,
}

/// Statistics about the memory service state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryServiceStats {
    pub total_messages: usize,
    pub recent_messages: usize,
    pub semantic_entries: usize,
    pub code_entries: usize,
    pub summary_entries: usize,
}

/// Statistics about routing decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingStats {
    pub total_messages: usize,
    pub semantic_only: usize,
    pub code_routed: usize,
    pub summary_routed: usize,
    pub skipped_low_salience: usize,
    pub storage_savings_percent: f32,
}

/// Routing decision for a message
#[derive(Debug)]
pub struct RoutingDecision {
    pub heads: Vec<EmbeddingHead>,
    pub should_embed: bool,
    pub skip_reason: Option<String>,
}

/// Request for generating a summary
#[derive(Debug)]
pub struct SummaryRequest {
    pub session_id: String,
    pub window_size: usize,
    pub summary_type: SummaryType,
}

/// Types of summaries that can be generated
#[derive(Debug, Clone)]
pub enum SummaryType {
    Rolling10,   // 10-message rolling summary
    Rolling100,  // 100-message mega summary
    Snapshot,    // On-demand snapshot summary
}

/// Parameters for memory recall operations
#[derive(Debug, Clone)]
pub struct RecallParams {
    pub session_id: String,
    pub query_embedding: Vec<f32>,
    pub k: usize,
    pub heads: Vec<EmbeddingHead>,
    pub min_salience: Option<f32>,
    pub max_age_hours: Option<i64>,
}

/// Result from a memory recall operation
#[derive(Debug)]
pub struct RecallResult {
    pub entries: Vec<ScoredMemoryEntry>,
    pub total_searched: usize,
    pub heads_searched: Vec<EmbeddingHead>,
    pub search_time_ms: u128,
}

/// Configuration for batch embedding operations
#[derive(Debug)]
pub struct BatchEmbeddingConfig {
    pub max_batch_size: usize,
    pub max_retries: usize,
    pub retry_delay_ms: u64,
}

impl Default for BatchEmbeddingConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 100,  // OpenAI optimal batch size
            max_retries: 3,
            retry_delay_ms: 1000,
        }
    }
}

/// Classification result with routing information
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub salience: f32,
    pub is_code: bool,
    pub lang: Option<String>,
    pub topics: Vec<String>,
    pub memory_type: MemoryType,
    pub suggested_heads: Vec<EmbeddingHead>,
}

/// Memory operation error types
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Database error: {0}")]
    Database(String),
    
    #[error("Vector store error: {0}")]
    VectorStore(String),
    
    #[error("LLM API error: {0}")]
    LlmApi(String),
    
    #[error("Classification error: {0}")]
    Classification(String),
    
    #[error("Embedding error: {0}")]
    Embedding(String),
    
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    
    #[error("Invalid configuration: {0}")]
    Configuration(String),
}

/// Convert from anyhow::Error to MemoryError
impl From<anyhow::Error> for MemoryError {
    fn from(err: anyhow::Error) -> Self {
        MemoryError::Database(err.to_string())
    }
}
