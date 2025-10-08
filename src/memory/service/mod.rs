// src/memory/service/mod.rs
use std::sync::Arc;
use anyhow::Result;
use tracing::info;

use crate::llm::client::OpenAIClient;
use crate::llm::provider::LlmProvider;
use crate::memory::{
    storage::sqlite::store::SqliteMemoryStore,
    storage::qdrant::multi_store::QdrantMultiStore,
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
        llm_provider: Arc<dyn LlmProvider>,
        embedding_client: Arc<OpenAIClient>,
    ) -> Self {
        info!("Initializing MemoryService with clean modular architecture");
        
        // Initialize core service
        let core = MemoryCoreService::new(
            sqlite_store.clone(),
            multi_store.clone(),
        );
        
        // Initialize MessagePipeline coordinator
        let message_pipeline = Arc::new(MessagePipeline::new(
            llm_provider.clone(),
        ));
        let message_pipeline_coordinator = MessagePipelineCoordinator::new(message_pipeline);
        
        // Initialize RecallEngine coordinator (needs both LLM and embeddings)
        let recall_engine = Arc::new(RecallEngine::new(
            llm_provider.clone(),      // For future chat-based features
            embedding_client.clone(),  // For embeddings in search
            sqlite_store.clone(),
            multi_store.clone(),
        ));
        let recall_engine_coordinator = RecallEngineCoordinator::new(recall_engine);
        
        // Initialize SummarizationEngine coordinator (needs both LLM and embeddings)
        let summarization_engine = Arc::new(SummarizationEngine::new(
            llm_provider.clone(),      // For summary generation via chat
            embedding_client.clone(),  // For summary embeddings
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
    
    /// Save a user message with analysis and return the entry ID
    /// 
    /// Returns the SQLite message ID (i64) directly - no unnecessary String conversion.
    /// This ID can be used to link responses or retrieve the message later.
    /// 
    /// FIXED: Removed project prefix logic - session is continuous regardless of project.
    /// project_id is metadata only, doesn't affect session_id.
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        _project_id: Option<&str>  // Kept for future metadata use
    ) -> Result<i64> {
        // Create memory entry with unchanged session_id (peter-eternal stays peter-eternal)
        let entry = MemoryEntry::user_message(session_id.to_string(), content.to_string());
        
        // Save via core service
        let entry_id = self.core.save_entry(&entry).await?;
        
        // Analyze via MessagePipeline coordinator  
        let analysis = self.message_pipeline.analyze_message(&entry, "user").await?;
        
        // Store analysis via core service
        self.core.store_analysis(entry_id, &analysis).await?;
        
        Ok(entry_id)
    }

    /// Save an assistant message and return its ID
    pub async fn save_assistant_message(
        &self,
        session_id: &str,
        content: &str,
        _in_reply_to: Option<i64>
    ) -> Result<i64> {
        let entry = MemoryEntry::assistant_message(session_id.to_string(), content.to_string());
        // Note: in_reply_to field not available on MemoryEntry, using parent_id instead if needed
        
        // Save via core service
        let entry_id = self.core.save_entry(&entry).await?;
        
        // Analyze via MessagePipeline coordinator  
        let analysis = self.message_pipeline.analyze_message(&entry, "assistant").await?;
        
        // Store analysis via core service
        self.core.store_analysis(entry_id, &analysis).await?;
        
        Ok(entry_id)
    }

    /// Get context for recall
    pub async fn get_context(&self, session_id: &str, query: &str) -> Result<RecallContext> {
        self.recall_engine.build_context(session_id, query).await
    }

    /// Parallel recall context building
    pub async fn parallel_recall_context(
        &self,
        session_id: &str,
        query: &str,
        recent_count: usize,
        semantic_count: usize
    ) -> Result<RecallContext> {
        self.recall_engine.parallel_recall_context(
            session_id,
            query,
            recent_count,
            semantic_count
        ).await
    }

    /// Get recent context
    pub async fn get_recent_context(&self, session_id: &str, count: usize) -> Result<Vec<MemoryEntry>> {
        self.recall_engine.get_recent_context(session_id, count).await
    }

    /// Search for similar memories
    pub async fn search_similar(
        &self,
        session_id: &str,
        query: &str,
        limit: usize
    ) -> Result<Vec<MemoryEntry>> {
        self.recall_engine.search_similar(session_id, query, limit).await
    }

    // ===== PHASE 1.2: NEW SUMMARY RETRIEVAL METHODS =====

    /// Get most recent rolling summary (last 100 messages, ~2,500 tokens)
    /// 
    /// Returns None if no rolling_100 summary exists for this session
    pub async fn get_rolling_summary(&self, session_id: &str) -> Result<Option<String>> {
        let pool = self.core.sqlite_store.get_pool();
        
        let result = sqlx::query!(
            r#"
            SELECT summary_text 
            FROM rolling_summaries 
            WHERE session_id = ? 
              AND summary_type = 'rolling_100'
            ORDER BY created_at DESC 
            LIMIT 1
            "#,
            session_id
        )
        .fetch_optional(pool)
        .await?;
        
        Ok(result.map(|r| r.summary_text))
    }

    /// Get most recent session summary (entire conversation, ~3,000 tokens)
    /// 
    /// Returns None if no snapshot summary exists for this session
    pub async fn get_session_summary(&self, session_id: &str) -> Result<Option<String>> {
        let pool = self.core.sqlite_store.get_pool();
        
        let result = sqlx::query!(
            r#"
            SELECT summary_text 
            FROM rolling_summaries 
            WHERE session_id = ? 
              AND summary_type = 'snapshot'
            ORDER BY created_at DESC 
            LIMIT 1
            "#,
            session_id
        )
        .fetch_optional(pool)
        .await?;
        
        Ok(result.map(|r| r.summary_text))
    }

    /// Count total messages in session
    /// 
    /// Utility method for determining if summaries should be generated
    pub async fn count_messages(&self, session_id: &str) -> Result<i64> {
        let pool = self.core.sqlite_store.get_pool();
        
        let result = sqlx::query!(
            r#"
            SELECT COUNT(*) as count 
            FROM memory_entries 
            WHERE session_id = ?
            "#,
            session_id
        )
        .fetch_one(pool)
        .await?;
        
        Ok(result.count as i64)
    }

    // ===== END PHASE 1.2 =====

    /// Create summary
    pub async fn create_summary(
        &self,
        session_id: &str,
        summary_type: SummaryType
    ) -> Result<String> {
        self.summarization_engine.create_summary(session_id, summary_type).await
    }

    /// Create rolling summary
    pub async fn create_rolling_summary(
        &self,
        session_id: &str,
        window_size: usize
    ) -> Result<String> {
        self.summarization_engine.create_rolling_summary(session_id, window_size).await
    }

    /// Create snapshot summary
    pub async fn create_snapshot_summary(
        &self,
        session_id: &str,
        context: Option<&str>
    ) -> Result<String> {
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
