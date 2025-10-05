// src/memory/features/recall_engine/mod.rs

//! Refactored RecallEngine with clean separation of concerns.
//! 
//! This replaces the monolithic recall_engine.rs with a modular architecture:
//! - search/ - Focused search strategy implementations  
//! - scoring/ - Dedicated scoring algorithms
//! - context/ - Context assembly logic
//! 
//! Same functionality, cleaner architecture, ready for code intelligence.

use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info};

use crate::llm::client::OpenAIClient;
use crate::llm::provider::LlmProvider;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::{
    core::types::MemoryEntry,
    storage::sqlite::store::SqliteMemoryStore,
    storage::qdrant::multi_store::QdrantMultiStore,
};

// Import our new focused modules
mod search;
mod scoring;
mod context;

// Re-export the implementations
use search::{RecentSearch, SemanticSearch, HybridSearch, MultiHeadSearch};
use context::MemoryContextBuilder;

// Public types are defined below, no need for re-export

// ===== PRESERVE ORIGINAL DATA STRUCTURES =====
// These stay exactly the same for backward compatibility

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
    /// Takes both LlmProvider (for future chat features) and OpenAIClient (for embeddings)
    pub fn new(
        _llm_provider: Arc<dyn LlmProvider>,
        embedding_client: Arc<OpenAIClient>,
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        // Build focused search strategies
        let recent_search = RecentSearch::new(sqlite_store.clone());
        let semantic_search = SemanticSearch::new(embedding_client.clone(), multi_store.clone());
        let hybrid_search = HybridSearch::new(
            recent_search.clone(),
            semantic_search.clone(),
        );
        let multihead_search = MultiHeadSearch::new(
            embedding_client.clone(),
            multi_store.clone(),
        );
        
        // Build context builder
        let context_builder = MemoryContextBuilder::new(
            hybrid_search.clone(),
        );
        
        Self {
            recent_search,
            semantic_search,
            hybrid_search,
            multihead_search,
            context_builder,
        }
    }

    /// Main entry point for all search operations - SAME API as before
    pub async fn search(
        &self,
        session_id: &str,
        mode: SearchMode,
    ) -> Result<Vec<ScoredMemory>> {
        debug!("RecallEngine::search - mode: {:?}", mode);
        
        match mode {
            SearchMode::Recent { limit } => {
                // Delegate to focused recent search strategy
                self.recent_search.search(session_id, limit).await
            }
            SearchMode::Semantic { query, limit } => {
                // Delegate to focused semantic search strategy
                self.semantic_search.search(session_id, &query, limit).await
            }
            SearchMode::Hybrid { query, config } => {
                // Delegate to focused hybrid search strategy
                self.hybrid_search.search(session_id, &query, &config).await
            }
            SearchMode::MultiHead { query, heads, limit } => {
                // Delegate to focused multihead search strategy
                self.multihead_search.search(session_id, &query, &heads, limit).await
            }
        }
    }

    /// Builds a recall context - SAME API as before
    /// 
    /// This preserves backward compatibility while using the new clean architecture
    pub async fn build_recall_context(
        &self,
        session_id: &str,
        query: &str,
        config: Option<RecallConfig>,
    ) -> Result<RecallContext> {
        info!("Building recall context for session: {}", session_id);
        
        // Delegate to the focused context builder
        let config = config.unwrap_or_default();
        self.context_builder.build_context(session_id, query, config).await
    }
}
