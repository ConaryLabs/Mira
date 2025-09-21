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
    // EXISTING API - DELEGATES TO MODULES
    // =====================================

    /// Store message analysis data  
    pub async fn store_analysis(&self, message_id: i64, analysis: &MessageAnalysis) -> Result<()> {
        self.analysis_ops.store_analysis(message_id, analysis).await
    }

    /// Load memories with analysis data (complex joins)
    pub async fn load_memories_with_analysis(&self, session_id: &str, n: usize) -> Result<Vec<MemoryEntry>> {
        self.analysis_ops.load_memories_with_analysis(session_id, n).await
    }

    /// Update recall metadata
    pub async fn update_recall_metadata(&self, message_id: i64) -> Result<()> {
        self.analysis_ops.update_recall_metadata(message_id).await
    }

    /// Save memory with explicit parent relationship
    pub async fn save_with_parent(&self, entry: &MemoryEntry, parent_id: Option<i64>) -> Result<MemoryEntry> {
        self.memory_ops.save_with_parent(entry, parent_id).await
    }

    /// Get active sessions
    pub async fn get_active_sessions(&self, hours: i64) -> Result<Vec<String>> {
        self.session_ops.get_active_sessions(hours).await
    }

    /// Update pin status
    pub async fn update_pin_status(&self, memory_id: i64, pinned: bool) -> Result<()> {
        self.session_ops.update_pin_status(memory_id, pinned).await
    }

    /// Embedding utility functions
    pub fn embedding_to_blob(embedding: &Option<Vec<f32>>) -> Option<Vec<u8>> {
        EmbeddingOperations::embedding_to_blob(embedding)
    }

    pub fn blob_to_embedding(blob: Option<Vec<u8>>) -> Option<Vec<f32>> {
        EmbeddingOperations::blob_to_embedding(blob)
    }
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    /// Delegate to memory operations module
    async fn save(&self, entry: &MemoryEntry) -> Result<MemoryEntry> {
        self.memory_ops.save_memory_entry(entry).await
    }

    /// Delegate to memory operations module
    async fn load_recent(&self, session_id: &str, n: usize) -> Result<Vec<MemoryEntry>> {
        self.memory_ops.load_recent_memories(session_id, n).await
    }

    /// Semantic search delegated to Qdrant (no change)
    async fn semantic_search(&self, _session_id: &str, _embedding: &[f32], _k: usize) -> Result<Vec<MemoryEntry>> {
        Ok(Vec::new()) // Qdrant handles this
    }

    /// Delegate to memory operations module
    async fn update_metadata(&self, id: i64, updated: &MemoryEntry) -> Result<MemoryEntry> {
        self.memory_ops.update_memory_metadata(id, updated).await
    }

    /// Delegate to memory operations module  
    async fn delete(&self, id: i64) -> Result<()> {
        self.memory_ops.delete_memory_entry(id).await
    }
}
