// src/memory/service/mod.rs
use std::sync::Arc;
use anyhow::Result;
use tracing::info;

use crate::llm::client::OpenAIClient;
use crate::memory::{
    storage::sqlite::store::SqliteMemoryStore,
    storage::qdrant::multi_store::QdrantMultiStore,
    cache::recent::RecentCache,
    core::types::MemoryEntry,
    features::{
        message_pipeline::MessagePipeline,
        recall_engine::RecallEngine,
        summarization::SummarizationEngine,
        memory_types::SummaryType,
        recall_engine::RecallContext,
    },
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
}

impl MemoryService {
    /// Creates a new memory service with clean modular architecture
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self::new_with_cache(sqlite_store, multi_store, llm_client, None)
    }
    
    pub fn new_with_cache(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
        recent_cache: Option<Arc<RecentCache>>,
    ) -> Self {
        info!("Initializing MemoryService with clean modular architecture");
        
        // Initialize core service
        let core = MemoryCoreService::new(
            sqlite_store.clone(),
            multi_store.clone(),
            recent_cache,
        );
        
        // Initialize MessagePipeline coordinator
        let message_pipeline = Arc::new(MessagePipeline::new(
            llm_client.clone(),
        ));
        let message_pipeline_coordinator = MessagePipelineCoordinator::new(message_pipeline);
        
        // Initialize RecallEngine coordinator
        let recall_engine = Arc::new(RecallEngine::new(
            llm_client.clone(),
            sqlite_store.clone(),
            multi_store.clone(),
        ));
        let recall_engine_coordinator = RecallEngineCoordinator::new(recall_engine);
        
        // Initialize SummarizationEngine coordinator
        let summarization_engine = Arc::new(SummarizationEngine::new(
            llm_client.clone(),
            sqlite_store.clone(),
            multi_store.clone(),
        ));
        let summarization_engine_coordinator = SummarizationEngineCoordinator::new(summarization_engine);
        
        info!("All memory service coordinators initialized successfully");
        
        Self {
            core,
            message_pipeline: message_pipeline_coordinator,
            recall_engine: recall_engine_coordinator,
            summarization_engine: summarization_engine_coordinator,
        }
    }
    
    /// Clean public API - saves a user message with analysis
    pub async fn save_user_message(&self, session_id: &str, content: &str, project_id: Option<&str>) -> Result<String> {
        // 1. Create memory entry - project scoping handled via session_id
        let effective_session_id = if let Some(pid) = project_id {
            format!("project-{}-{}", pid, session_id)
        } else {
            session_id.to_string()
        };
        
        let entry = MemoryEntry::user_message(effective_session_id, content.to_string());
        
        // 2. Save via core service
        let entry_id = self.core.save_entry(&entry).await?;
        
        // 3. Analyze via MessagePipeline coordinator  
        let analysis = self.message_pipeline.analyze_message(&entry, "user").await?;
        
        // 4. Store analysis via core service
        self.core.store_analysis(entry_id, &analysis).await?;
        
        Ok(entry_id.to_string())
    }

    /// Save assistant response with project_id
    pub async fn save_assistant_response(&self, session_id: &str, response: &crate::llm::types::ChatResponse, project_id: Option<&str>) -> Result<String> {
        // 1. Create memory entry from ChatResponse content with project_id
        let effective_session_id = if let Some(pid) = project_id {
            format!("project-{}-{}", pid, session_id)
        } else {
            session_id.to_string()
        };
        
        let entry = MemoryEntry::assistant_message(effective_session_id, response.output.clone());
        
        // 2. Save via core service
        let entry_id = self.core.save_entry(&entry).await?;
        
        // 3. Analyze via MessagePipeline coordinator  
        let analysis = self.message_pipeline.analyze_message(&entry, "assistant").await?;
        
        // 4. Store analysis via core service
        self.core.store_analysis(entry_id, &analysis).await?;
        
        Ok(entry_id.to_string())
    }

    /// Get context for recall
    pub async fn get_context(&self, session_id: &str, query: &str) -> Result<RecallContext> {
        self.recall_engine.build_context(session_id, query).await
    }

    /// Parallel recall context building
    pub async fn parallel_recall_context(&self, session_id: &str, query: &str, recent_count: usize, semantic_count: usize) -> Result<RecallContext> {
        self.recall_engine.parallel_recall_context(session_id, query, recent_count, semantic_count).await
    }

    /// Get recent context
    pub async fn get_recent_context(&self, session_id: &str, count: usize) -> Result<Vec<MemoryEntry>> {
        self.recall_engine.get_recent_context(session_id, count).await
    }

    /// Search for similar memories
    pub async fn search_similar(&self, session_id: &str, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.recall_engine.search_similar(session_id, query, limit).await
    }

    /// Create summary
    pub async fn create_summary(&self, session_id: &str, summary_type: SummaryType) -> Result<String> {
        self.summarization_engine.create_summary(session_id, summary_type).await
    }

    /// Create rolling summary
    pub async fn create_rolling_summary(&self, session_id: &str, window_size: usize) -> Result<String> {
        self.summarization_engine.create_rolling_summary(session_id, window_size).await
    }

    /// Create snapshot summary
    pub async fn create_snapshot_summary(&self, session_id: &str, context: Option<&str>) -> Result<String> {
        self.summarization_engine.create_snapshot_summary(session_id, context).await
    }

    /// Get service stats
    pub async fn get_stats(&self, session_id: &str) -> Result<serde_json::Value> {
        self.core.get_stats(session_id).await
    }

    /// Cleanup inactive sessions
    pub async fn cleanup_inactive_sessions(&self, max_age_hours: i64) -> Result<usize> {
        self.core.cleanup_inactive_sessions(max_age_hours).await
    }
}
