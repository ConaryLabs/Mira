// src/memory/service/recall_engine/coordinator.rs

use crate::memory::{
    core::types::MemoryEntry,
    features::recall_engine::{RecallConfig, RecallContext, RecallEngine, SearchMode},
};
use anyhow::Result;
use std::sync::Arc;

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
        self.engine
            .build_context(session_id, Some(query.to_string()), self.config.clone())
            .await
    }

    pub async fn parallel_recall_context(
        &self,
        session_id: &str,
        query: &str,
        recent_count: usize,
        semantic_count: usize,
    ) -> Result<RecallContext> {
        // Use the hybrid search with custom config
        let config = RecallConfig {
            recent_count,
            semantic_count,
            ..self.config.clone()
        };
        self.engine
            .build_context(session_id, Some(query.to_string()), config)
            .await
    }

    pub async fn get_recent_context(
        &self,
        session_id: &str,
        count: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // Use the search method with Recent mode
        let scored_memories = self
            .engine
            .search(session_id, SearchMode::Recent { limit: count })
            .await?;
        Ok(scored_memories
            .into_iter()
            .map(|scored| scored.entry)
            .collect())
    }

    pub async fn search_similar(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // Use the search method with Semantic mode
        let scored_memories = self
            .engine
            .search(
                session_id,
                SearchMode::Semantic {
                    query: query.to_string(),
                    limit,
                },
            )
            .await?;
        Ok(scored_memories
            .into_iter()
            .map(|scored| scored.entry)
            .collect())
    }
}
