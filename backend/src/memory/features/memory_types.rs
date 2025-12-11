// src/memory/features/memory_types.rs
// Shared types and structs for the memory service modules

use crate::llm::EmbeddingHead;
use crate::memory::core::types::MemoryEntry;
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Copy)]
pub enum SummaryType {
    Rolling, // 100-message rolling summary
    Snapshot, // On-demand snapshot summary
}

/// Record structure for summary retrieval from rolling_summaries table
#[derive(Debug, Clone)]
pub struct SummaryRecord {
    pub id: i64,
    pub summary_type: String,
    pub summary_text: String,
    pub message_count: usize,
    pub created_at: i64,
}

impl SummaryRecord {
    /// Convert to display-friendly format
    pub fn to_display(&self) -> String {
        let summary_type = match self.summary_type.as_str() {
            "rolling" => "Rolling",
            "snapshot" => "Snapshot",
            _ => &self.summary_type,
        };

        format!(
            "[{} Summary] {} messages: {}",
            summary_type,
            self.message_count,
            self.summary_text.chars().take(100).collect::<String>()
        )
    }

    /// Get age in hours
    pub fn age_hours(&self) -> f64 {
        let now = chrono::Utc::now().timestamp();
        (now - self.created_at) as f64 / 3600.0
    }
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
            max_batch_size: 10,
            max_retries: 3,
            retry_delay_ms: 1000,
        }
    }
}
