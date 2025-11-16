// src/memory/features/recall_engine/mod.rs

//! Refactored RecallEngine with clean separation of concerns.
//!
//! This replaces the monolithic recall_engine.rs with a modular architecture:
//! - search/ - Focused search strategy implementations  
//! - scoring/ - Dedicated scoring algorithms
//! - context/ - Context assembly logic
//!
//! Same functionality, cleaner architecture, ready for code intelligence.

use anyhow::Result;
use std::sync::Arc;
use tracing::debug;

use crate::llm::embeddings::EmbeddingHead;
use crate::llm::provider::{LlmProvider, OpenAiEmbeddings};
use crate::memory::{
    core::types::MemoryEntry, storage::qdrant::multi_store::QdrantMultiStore,
    storage::sqlite::store::SqliteMemoryStore,
};

// Import our new focused modules
mod context;
mod scoring;
mod search;

// Re-export the implementations
use context::MemoryContextBuilder;
use search::{HybridSearch, MultiHeadSearch, RecentSearch, SemanticSearch};

// Public types are defined below, no need for re-export

// ===== PRESERVE ORIGINAL DATA STRUCTURES =====
// These stay exactly the same for backward compatibility

/// Context for recall operations containing recent and semantic memories
///
/// PHASE 1.1 UPDATE: Added summary fields for layered context architecture
#[derive(Debug, Clone)]
pub struct RecallContext {
    // Original fields - keep for backward compatibility
    pub recent: Vec<MemoryEntry>,
    pub semantic: Vec<MemoryEntry>,

    // NEW: Summary layers for generous context
    /// Rolling summary of last 100 messages (~2,500 tokens)
    /// Provides mid-range context without full message history
    pub rolling_summary: Option<String>,

    /// Session-level snapshot summary (~3,000 tokens)
    /// Comprehensive overview of entire conversation
    pub session_summary: Option<String>,
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
    MultiHead {
        query: String,
        heads: Vec<EmbeddingHead>,
        limit: usize,
    },
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

// ===== NEW CLEAN RECALL ENGINE =====

/// Unified recall engine - now clean and modular!
pub struct RecallEngine {
    // Search strategies - focused, single-purpose modules
    recent_search: RecentSearch,
    semantic_search: SemanticSearch,
    hybrid_search: HybridSearch,
    multihead_search: MultiHeadSearch,

    // Context building - clean separation from search logic
    context_builder: MemoryContextBuilder,
}

impl RecallEngine {
    /// Creates a new recall engine with clean modular architecture
    /// Takes both LlmProvider (for future chat features) and OpenAiEmbeddings (for embeddings)
    pub fn new(
        _llm_provider: Arc<dyn LlmProvider>,
        embedding_client: Arc<OpenAiEmbeddings>,
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        // Build focused search strategies
        let recent_search = RecentSearch::new(sqlite_store.clone());
        let semantic_search = SemanticSearch::new(embedding_client.clone(), multi_store.clone());
        let hybrid_search = HybridSearch::new(recent_search.clone(), semantic_search.clone());
        let multihead_search = MultiHeadSearch::new(embedding_client.clone(), multi_store.clone());

        // Build context builder
        let context_builder = MemoryContextBuilder::new(hybrid_search.clone());

        Self {
            recent_search,
            semantic_search,
            hybrid_search,
            multihead_search,
            context_builder,
        }
    }

    /// Main entry point for all search operations - SAME API as before
    pub async fn search(&self, session_id: &str, mode: SearchMode) -> Result<Vec<ScoredMemory>> {
        debug!("RecallEngine::search - mode: {:?}", mode);

        match mode {
            SearchMode::Recent { limit } => self.recent_search.search(session_id, limit).await,
            SearchMode::Semantic { query, limit } => {
                self.semantic_search.search(session_id, &query, limit).await
            }
            SearchMode::Hybrid { query, config } => {
                self.hybrid_search.search(session_id, &query, &config).await
            }
            SearchMode::MultiHead {
                query,
                heads,
                limit,
            } => {
                self.multihead_search
                    .search(session_id, &query, &heads, limit)
                    .await
            }
        }
    }

    /// Build context for prompt construction - delegates to context builder
    pub async fn build_context(
        &self,
        session_id: &str,
        query: Option<String>,
        config: RecallConfig,
    ) -> Result<RecallContext> {
        // Convert Option<String> to &str for context builder
        let query_str = query.as_deref().unwrap_or("");
        self.context_builder
            .build_context(session_id, query_str, config)
            .await
    }

    /// Get engine statistics
    pub fn get_stats(&self) -> String {
        "RecallEngine: Recent, Semantic, Hybrid, MultiHead strategies active".to_string()
    }
}
