// src/memory/service/recall_engine/coordinator.rs
use std::sync::Arc;
use anyhow::Result;
use crate::memory::{
    features::recall_engine::{RecallEngine, RecallContext, RecallConfig, SearchMode, ScoredMemory},
    core::types::MemoryEntry,
};

pub struct RecallEngineCoordinator {
    engine: Arc<RecallEngine>,
    config: RecallConfig,
}

impl RecallEngineCoordinator {
    pub fn new(engine: Arc<RecallEngine>) -> Self {
        Self {
            engine,
            config: RecallConfig::default(),
        }
    }

    pub async fn build_context(&self, session_id: &str, query: &str) -> Result<RecallContext> {
        self.engine.build_recall_context(session_id, query, Some(self.config.clone())).await
    }

    pub async fn parallel_recall_context(&self, session_id: &str, query: &str, recent_count: usize, semantic_count: usize) -> Result<RecallContext> {
        // Use the hybrid search with custom config
        let config = RecallConfig {
            recent_count,
            semantic_count,
            ..self.config.clone()
        };
        self.engine.build_recall_context(session_id, query, Some(config)).await
    }

    pub async fn get_recent_context(&self, session_id: &str, count: usize) -> Result<Vec<MemoryEntry>> {
        // Use the search method with Recent mode
        let scored_memories = self.engine.search(session_id, SearchMode::Recent { limit: count }).await?;
        Ok(scored_memories.into_iter().map(|scored| scored.entry).collect())
    }

    pub async fn search_similar(&self, session_id: &str, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        // Use the search method with Semantic mode
        let scored_memories = self.engine.search(session_id, SearchMode::Semantic { 
            query: query.to_string(), 
            limit 
        }).await?;
        Ok(scored_memories.into_iter().map(|scored| scored.entry).collect())
    }

    // Future: Code context building
    pub async fn build_code_context(&self, _query: &str, _file_path: Option<&str>) -> Result<RecallContext> {
        // Will delegate to code context builder when we implement code intelligence
        todo!("Future integration point")
    }
}
