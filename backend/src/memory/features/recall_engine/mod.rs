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
use tracing::{debug, info};

use crate::context_oracle::{ContextConfig, ContextOracle, ContextRequest, GatheredContext};
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
/// Combines conversation memory with optional code intelligence from the Context Oracle.
#[derive(Debug, Clone)]
pub struct RecallContext {
    // Conversation memory
    pub recent: Vec<MemoryEntry>,
    pub semantic: Vec<MemoryEntry>,

    // Summary layers for generous context
    /// Rolling summary of last 100 messages (~2,500 tokens)
    pub rolling_summary: Option<String>,
    /// Session-level snapshot summary (~3,000 tokens)
    pub session_summary: Option<String>,

    // Code intelligence from Context Oracle
    /// Gathered context from code intelligence systems (semantic code search,
    /// call graph, co-change, historical fixes, patterns, build errors, expertise)
    pub code_intelligence: Option<GatheredContext>,
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
///
/// Combines conversation memory search with optional code intelligence from ContextOracle.
pub struct RecallEngine {
    // Search strategies - focused, single-purpose modules
    recent_search: RecentSearch,
    semantic_search: SemanticSearch,
    hybrid_search: HybridSearch,
    multihead_search: MultiHeadSearch,

    // Context building - clean separation from search logic
    context_builder: MemoryContextBuilder,

    // Optional code intelligence oracle
    context_oracle: Option<Arc<ContextOracle>>,
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
            context_oracle: None,
        }
    }

    /// Add a context oracle for code intelligence integration
    pub fn with_oracle(mut self, oracle: Arc<ContextOracle>) -> Self {
        self.context_oracle = Some(oracle);
        self
    }

    /// Set the context oracle (for when engine is already constructed)
    pub fn set_oracle(&mut self, oracle: Arc<ContextOracle>) {
        self.context_oracle = Some(oracle);
    }

    /// Check if oracle is available
    pub fn has_oracle(&self) -> bool {
        self.context_oracle.is_some()
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

    /// Build enriched context combining memory recall with code intelligence
    ///
    /// This method gathers both conversation memory and code intelligence from the
    /// Context Oracle in a single call, providing comprehensive context for LLM prompts.
    pub async fn build_enriched_context(
        &self,
        session_id: &str,
        query: &str,
        recall_config: RecallConfig,
        oracle_config: Option<ContextConfig>,
        project_id: Option<&str>,
        current_file: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<RecallContext> {
        info!(
            "Building enriched context for session {} with oracle: {}",
            session_id,
            self.has_oracle()
        );

        // Get memory context first
        let mut context = self
            .context_builder
            .build_context(session_id, query, recall_config)
            .await?;

        // Add code intelligence if oracle is available
        if let Some(oracle) = &self.context_oracle {
            let oracle_cfg = oracle_config.unwrap_or_default();

            // Build oracle request
            let mut request = ContextRequest::new(query.to_string(), session_id.to_string())
                .with_config(oracle_cfg);

            if let Some(pid) = project_id {
                request = request.with_project(pid);
            }

            if let Some(file) = current_file {
                request = request.with_file(file);
            }

            if let Some(error) = error_message {
                request = request.with_error(error, None);
            }

            // Gather code intelligence
            match oracle.gather(&request).await {
                Ok(gathered) => {
                    if !gathered.is_empty() {
                        info!(
                            "Gathered code intelligence: {} sources, ~{} tokens",
                            gathered.sources_used.len(),
                            gathered.estimated_tokens
                        );
                        context.code_intelligence = Some(gathered);
                    }
                }
                Err(e) => {
                    // Log but don't fail - code intelligence is optional
                    tracing::warn!("Failed to gather code intelligence: {}", e);
                }
            }
        }

        Ok(context)
    }

    /// Build context with code intelligence using default oracle config
    pub async fn build_context_with_oracle(
        &self,
        session_id: &str,
        query: &str,
        recall_config: RecallConfig,
        project_id: Option<&str>,
        current_file: Option<&str>,
    ) -> Result<RecallContext> {
        self.build_enriched_context(
            session_id,
            query,
            recall_config,
            None,
            project_id,
            current_file,
            None,
        )
        .await
    }

    /// Get engine statistics
    pub fn get_stats(&self) -> String {
        "RecallEngine: Recent, Semantic, Hybrid, MultiHead strategies active".to_string()
    }
}
