// src/memory/service/mod.rs
use std::sync::Arc;
use tracing::info;

use crate::memory::context::{ContextConfig, ContextOracle};
use crate::llm::provider::{EmbeddingProvider, LlmProvider};
use crate::memory::{
    features::{
        message_pipeline::MessagePipeline,
        recall_engine::{RecallContext, RecallEngine},
        summarization::SummarizationEngine,
    },
    storage::qdrant::multi_store::QdrantMultiStore,
    storage::sqlite::store::SqliteMemoryStore,
};

// Module declarations
pub mod core_service;
pub mod message_pipeline;
pub mod recall_engine;
pub mod summarization_engine;

// Re-exports
pub use core_service::MemoryCoreService;
pub use message_pipeline::coordinator::MessagePipelineCoordinator;
pub use recall_engine::coordinator::RecallEngineCoordinator;
pub use summarization_engine::coordinator::SummarizationEngineCoordinator;

/// Main Memory Service - Clean delegation to specialized coordinators
pub struct MemoryService {
    // Core storage operations
    pub core: MemoryCoreService,

    // Pipeline coordinators - map directly to our 3 engines
    pub message_pipeline: MessagePipelineCoordinator,
    pub recall_engine: RecallEngineCoordinator,
    pub summarization_engine: SummarizationEngineCoordinator,

    // Keep reference to multi_store for backfill and other operations
    multi_store: Arc<QdrantMultiStore>,

    // Keep reference to embedding client for direct embedding operations
    embedding_client: Arc<dyn EmbeddingProvider>,
}

impl MemoryService {
    /// Creates a new memory service with clean modular architecture
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_provider: Arc<dyn LlmProvider>,
        embedding_client: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        Self::with_oracle(sqlite_store, multi_store, llm_provider, embedding_client, None)
    }

    /// Creates a new memory service with optional context oracle for code intelligence
    pub fn with_oracle(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_provider: Arc<dyn LlmProvider>,
        embedding_client: Arc<dyn EmbeddingProvider>,
        context_oracle: Option<Arc<ContextOracle>>,
    ) -> Self {
        info!(
            "Initializing MemoryService with 3 unified engines (oracle: {})",
            context_oracle.is_some()
        );

        // Core storage with multi_store for get_multi_store access
        let core = MemoryCoreService::new(sqlite_store.clone(), multi_store.clone());

        // Initialize 3 unified engines - MessagePipeline only needs LLM provider
        let message_pipeline = Arc::new(MessagePipeline::new(llm_provider.clone()));

        // RecallEngine with optional oracle for code intelligence
        let mut recall_engine = RecallEngine::new(
            llm_provider.clone(),
            embedding_client.clone(),
            sqlite_store.clone(),
            multi_store.clone(),
        );

        if let Some(oracle) = context_oracle {
            recall_engine.set_oracle(oracle);
        }

        let recall_engine = Arc::new(recall_engine);

        let summarization_engine = Arc::new(SummarizationEngine::new(
            llm_provider.clone(),
            embedding_client.clone(),
            sqlite_store.clone(),
            multi_store.clone(),
        ));

        // Wrap in coordinators for clean interface
        let message_pipeline_coordinator = MessagePipelineCoordinator::new(message_pipeline);
        let recall_engine_coordinator = RecallEngineCoordinator::new(recall_engine);
        let summarization_engine_coordinator =
            SummarizationEngineCoordinator::new(summarization_engine);

        info!("MemoryService initialized successfully");

        Self {
            core,
            message_pipeline: message_pipeline_coordinator,
            recall_engine: recall_engine_coordinator,
            summarization_engine: summarization_engine_coordinator,
            multi_store: multi_store.clone(),
            embedding_client: embedding_client.clone(),
        }
    }

    /// Direct access to multi-store for special operations
    pub fn get_multi_store(&self) -> Arc<QdrantMultiStore> {
        self.multi_store.clone()
    }

    /// Direct access to embedding client for embedding operations
    pub fn get_embedding_client(&self) -> Arc<dyn EmbeddingProvider> {
        self.embedding_client.clone()
    }

    /// Get service statistics  
    pub fn get_stats(&self) -> String {
        format!(
            "MemoryService Stats:\n- MessagePipeline active\n- RecallEngine active\n- SummarizationEngine active"
        )
    }

    // ===== DELEGATION METHODS FOR BACKWARD COMPATIBILITY =====

    // Core service delegations
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> anyhow::Result<i64> {
        self.core
            .save_user_message(session_id, content, project_id)
            .await
    }

    pub async fn save_assistant_message(
        &self,
        session_id: &str,
        content: &str,
        parent_id: Option<i64>,
    ) -> anyhow::Result<i64> {
        self.core
            .save_assistant_message(session_id, content, parent_id)
            .await
    }

    // Recall engine delegations
    pub async fn parallel_recall_context(
        &self,
        session_id: &str,
        query: &str,
        recent_count: usize,
        semantic_count: usize,
        project_id: Option<&str>,
    ) -> anyhow::Result<RecallContext> {
        self.recall_engine
            .parallel_recall_context(session_id, query, recent_count, semantic_count, project_id)
            .await
    }

    /// Build enriched context combining memory recall with code intelligence
    ///
    /// This is the primary method for getting comprehensive context that includes
    /// both conversation memory and code intelligence from the Context Oracle.
    pub async fn build_enriched_context(
        &self,
        session_id: &str,
        query: &str,
        project_id: Option<&str>,
        current_file: Option<&str>,
    ) -> anyhow::Result<RecallContext> {
        self.recall_engine
            .build_enriched_context(session_id, query, project_id, current_file)
            .await
    }

    /// Build enriched context with custom oracle configuration
    pub async fn build_enriched_context_with_config(
        &self,
        session_id: &str,
        query: &str,
        oracle_config: ContextConfig,
        project_id: Option<&str>,
        current_file: Option<&str>,
        error_message: Option<&str>,
    ) -> anyhow::Result<RecallContext> {
        self.recall_engine
            .build_enriched_context_with_config(
                session_id,
                query,
                oracle_config,
                project_id,
                current_file,
                error_message,
            )
            .await
    }

    /// Build parallel recall context with code intelligence
    pub async fn parallel_recall_context_with_oracle(
        &self,
        session_id: &str,
        query: &str,
        recent_count: usize,
        semantic_count: usize,
        project_id: Option<&str>,
        current_file: Option<&str>,
    ) -> anyhow::Result<RecallContext> {
        self.recall_engine
            .parallel_recall_context_with_oracle(
                session_id,
                query,
                recent_count,
                semantic_count,
                project_id,
                current_file,
            )
            .await
    }

    /// Check if oracle is available for code intelligence
    pub fn has_oracle(&self) -> bool {
        self.recall_engine.has_oracle()
    }

    // Summarization engine delegations
    pub async fn get_rolling_summary(&self, session_id: &str) -> anyhow::Result<Option<String>> {
        self.summarization_engine
            .get_rolling_summary(session_id)
            .await
    }

    pub async fn get_session_summary(&self, session_id: &str) -> anyhow::Result<Option<String>> {
        self.summarization_engine
            .get_session_summary(session_id)
            .await
    }

    // Core cleanup delegation
    pub async fn cleanup_inactive_sessions(&self, max_age_hours: i64) -> anyhow::Result<usize> {
        self.core.cleanup_inactive_sessions(max_age_hours).await
    }
}
