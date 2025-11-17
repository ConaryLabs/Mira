// src/memory/storage/sqlite/store.rs
// Clean SqliteMemoryStore

use crate::memory::core::traits::MemoryStore;
use crate::memory::core::types::MemoryEntry;
use anyhow::Result;
use async_trait::async_trait;
use sqlx::SqlitePool;
use tracing::info;

use super::core::{AnalysisOperations, EmbeddingOperations, MemoryOperations, MessageAnalysis};

/// Clean SqliteMemoryStore that delegates to focused operation modules
pub struct SqliteMemoryStore {
    pub pool: SqlitePool,

    // Operation modules - each handles specific concerns
    memory_ops: MemoryOperations,
    analysis_ops: AnalysisOperations,
    embedding_ops: EmbeddingOperations,
}

impl SqliteMemoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            memory_ops: MemoryOperations::new(pool.clone()),
            analysis_ops: AnalysisOperations::new(pool.clone()),
            embedding_ops: EmbeddingOperations::new(pool.clone()),
            pool,
        }
    }

    /// Get access to the underlying SQLite pool for direct queries
    /// Used by SummaryStorage to access rolling_summaries table
    pub fn get_pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Database migrations (unchanged)
    pub async fn run_migrations(&self) -> Result<()> {
        info!("Migrations handled by SQLx CLI");
        Ok(())
    }

    // =====================================
    // BASIC FUNCTIONALITY
    // =====================================

    /// Store message analysis from user input or assistant response
    pub async fn store_analysis(&self, message_id: i64, analysis: &MessageAnalysis) -> Result<()> {
        self.analysis_ops.store_analysis(message_id, analysis).await
    }

    /// Store embedding reference
    pub async fn store_embedding_reference(
        &self,
        message_id: i64,
        embedding_heads: &[String],
    ) -> Result<()> {
        self.embedding_ops
            .store_embedding_reference(message_id, embedding_heads)
            .await
    }
}

// Core MemoryStore trait implementation - delegates to memory_ops
#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    async fn save(&self, entry: &MemoryEntry) -> Result<MemoryEntry> {
        self.memory_ops.save_memory_entry(entry).await
    }

    async fn load_recent(&self, session_id: &str, n: usize) -> Result<Vec<MemoryEntry>> {
        self.memory_ops.load_recent_memories(session_id, n).await
    }

    async fn semantic_search(
        &self,
        session_id: &str,
        _embedding: &[f32],
        k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // SQLite doesn't handle semantic search - return recent as fallback
        self.load_recent(session_id, k).await
    }

    async fn update_metadata(&self, id: i64, entry: &MemoryEntry) -> Result<MemoryEntry> {
        self.memory_ops.update_memory_metadata(id, entry).await
    }

    async fn delete(&self, id: i64) -> Result<()> {
        self.memory_ops.delete_memory_entry(id).await
    }
}
