// src/memory/service/core_service.rs - Add missing imports at the top
use std::sync::Arc;
use anyhow::Result;
use crate::memory::{
    storage::sqlite::store::SqliteMemoryStore,
    storage::qdrant::multi_store::QdrantMultiStore,
    core::{
        types::MemoryEntry,
        traits::MemoryStore,  // <- Add this import
    },
    features::UnifiedAnalysis,  // <- Add this import
};

pub struct MemoryCoreService {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub multi_store: Arc<QdrantMultiStore>,
}

impl MemoryCoreService {
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            sqlite_store,
            multi_store,
        }
    }

    /// Save a memory entry and return the entry ID
    pub async fn save_entry(&self, entry: &MemoryEntry) -> Result<i64> {
        let saved_entry = self.sqlite_store.save(entry).await?;
        Ok(saved_entry.id.unwrap_or(0)) // Handle Option<i64>
    }

    /// Get recent memories for a session
    pub async fn get_recent(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.sqlite_store.load_recent(session_id, limit).await // usize, not i32
    }

    /// Store analysis results for an entry
    pub async fn store_analysis(&self, _entry_id: i64, _analysis: &UnifiedAnalysis) -> Result<()> {
        // Store analysis metadata in SQLite
        // Store embedding vectors in Qdrant via multi-store
        // This is where we'd implement analysis storage
        // For now, just return Ok - we'll implement this properly as we rebuild
        Ok(())
    }

    /// Get service statistics
    pub async fn get_stats(&self, session_id: &str) -> Result<serde_json::Value> {
        // Return basic stats - we'll expand this
        Ok(serde_json::json!({
            "session_id": session_id,
            "status": "operational"
        }))
    }

    /// Cleanup inactive sessions
    pub async fn cleanup_inactive_sessions(&self, _max_age_hours: i64) -> Result<usize> {
        // Cleanup logic will be implemented here
        // For now, return 0
        Ok(0)
    }
}
