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

use crate::memory::features::{
    recall_engine::{RecallContext, RecallConfig},
};

/// Main Memory Service - Clean delegation to specialized coordinators
pub struct MemoryService {
    // Core storage operations
    pub core: MemoryCoreService,
    
    // Pipeline coordinators - map directly to our 3 engines
    pub message_pipeline: MessagePipelineCoordinator,
    // TODO: pub recall_engine: RecallEngineCoordinator,
    // TODO: pub summarization_engine: SummarizationEngineCoordinator,
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
            sqlite_store.clone(),
        ));
        let message_pipeline_coordinator = MessagePipelineCoordinator::new(message_pipeline);
        
        info!("All memory service coordinators initialized successfully");
        
        Self {
            core,
            message_pipeline: message_pipeline_coordinator,
        }
    }
    
    /// Clean public API - saves a user message with analysis
    pub async fn save_user_message(&self, session_id: &str, content: &str) -> Result<String> {
        // 1. Create memory entry
        let entry = MemoryEntry::user_message(session_id.to_string(), content.to_string());
        
        // 2. Save via core service
        let entry_id = self.core.save_entry(&entry).await?;
        
        // 3. Analyze via MessagePipeline coordinator  
        let analysis = self.message_pipeline.analyze_message(&entry, "user").await?;
        
        // 4. Store analysis via core service
        self.core.store_analysis(entry_id, &analysis).await?;
        
        Ok(entry_id.to_string())
    }
    
    // TODO: Add other public methods as we implement coordinators
    // pub async fn get_context(&self, session_id: &str, query: &str) -> Result<RecallContext> { ... }
    // pub async fn create_summary(&self, session_id: &str, summary_type: SummaryType) -> Result<String> { ... }
}
