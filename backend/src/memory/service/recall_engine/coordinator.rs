// src/memory/service/recall_engine/coordinator.rs

use crate::memory::context::ContextConfig;
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

    /// Check if the underlying engine has a context oracle configured
    pub fn has_oracle(&self) -> bool {
        self.engine.has_oracle()
    }

    pub async fn build_context(&self, session_id: &str, query: &str) -> Result<RecallContext> {
        self.engine
            .build_context(session_id, Some(query.to_string()), self.config.clone())
            .await
    }

    /// Build context with code intelligence from the Context Oracle
    ///
    /// This combines conversation memory with code intelligence for comprehensive context.
    pub async fn build_enriched_context(
        &self,
        session_id: &str,
        query: &str,
        project_id: Option<&str>,
        current_file: Option<&str>,
    ) -> Result<RecallContext> {
        self.engine
            .build_context_with_oracle(
                session_id,
                query,
                self.config.clone(),
                project_id,
                current_file,
            )
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
    ) -> Result<RecallContext> {
        self.engine
            .build_enriched_context(
                session_id,
                query,
                self.config.clone(),
                Some(oracle_config),
                project_id,
                current_file,
                error_message,
            )
            .await
    }

    pub async fn parallel_recall_context(
        &self,
        session_id: &str,
        query: &str,
        recent_count: usize,
        semantic_count: usize,
        project_id: Option<&str>,
    ) -> Result<RecallContext> {
        // Use the hybrid search with custom config including project for boosting
        let config = RecallConfig {
            recent_count,
            semantic_count,
            current_project_id: project_id.map(|s| s.to_string()),
            ..self.config.clone()
        };
        self.engine
            .build_context(session_id, Some(query.to_string()), config)
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
    ) -> Result<RecallContext> {
        let config = RecallConfig {
            recent_count,
            semantic_count,
            current_project_id: project_id.map(|s| s.to_string()),
            ..self.config.clone()
        };
        self.engine
            .build_context_with_oracle(session_id, query, config, project_id, current_file)
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
