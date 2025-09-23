// src/memory/storage/sqlite/store.rs
// Clean SqliteMemoryStore with structured response support

use crate::memory::core::traits::MemoryStore;
use crate::memory::core::types::MemoryEntry;
use crate::llm::structured::CompleteResponse;  // NEW: Import structured types
use anyhow::Result;
use async_trait::async_trait;
use sqlx::SqlitePool;
use tracing::info;

use super::core::{
    MemoryOperations,
    AnalysisOperations, 
    SessionOperations,
    EmbeddingOperations,
    MessageAnalysis,
};

/// Clean SqliteMemoryStore that delegates to focused operation modules
pub struct SqliteMemoryStore {
    pub pool: SqlitePool,
    
    // Operation modules - each handles specific concerns
    memory_ops: MemoryOperations,
    analysis_ops: AnalysisOperations,
    session_ops: SessionOperations,
    embedding_ops: EmbeddingOperations,
}

impl SqliteMemoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            memory_ops: MemoryOperations::new(pool.clone()),
            analysis_ops: AnalysisOperations::new(pool.clone()),
            session_ops: SessionOperations::new(pool.clone()),
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
    // NEW: STRUCTURED RESPONSE OPERATIONS
    // =====================================

    /// Save complete structured response atomically to all 3 tables
    /// This is the new way to save assistant responses with full metadata
    pub async fn save_structured_response(
        &self,
        session_id: &str,
        response: &CompleteResponse,
        parent_id: Option<i64>,
    ) -> Result<i64> {
        super::structured_ops::save_structured_response(&self.pool, session_id, response, parent_id).await
    }

    /// Load complete structured response by message ID
    pub async fn load_structured_response(&self, message_id: i64) -> Result<Option<CompleteResponse>> {
        super::structured_ops::load_structured_response(&self.pool, message_id).await
    }

    /// Get statistics about structured responses
    pub async fn get_structured_response_stats(&self) -> Result<super::structured_ops::StructuredResponseStats> {
        super::structured_ops::get_structured_response_stats(&self.pool).await
    }

    // =====================================
    // EXISTING API - BASIC FUNCTIONALITY
    // =====================================

    /// Store message analysis from user input or assistant response
    pub async fn store_analysis(&self, message_id: i64, analysis: &MessageAnalysis) -> Result<()> {
        self.analysis_ops.store_analysis(message_id, analysis).await
    }

    /// Store embedding reference
    pub async fn store_embedding_reference(&self, message_id: i64, embedding_heads: &[String]) -> Result<()> {
        self.embedding_ops.store_embedding_reference(message_id, embedding_heads).await
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

    async fn semantic_search(&self, session_id: &str, _embedding: &[f32], k: usize) -> Result<Vec<MemoryEntry>> {
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
