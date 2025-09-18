// src/memory/service/core_service.rs
use std::sync::Arc;
use anyhow::Result;
use crate::memory::{
    storage::sqlite::store::SqliteMemoryStore,
    storage::qdrant::multi_store::QdrantMultiStore,
    cache::recent::RecentCache,
    core::types::MemoryEntry,
};

pub struct MemoryCoreService {
    pub(crate) sqlite_store: Arc<SqliteMemoryStore>,
    pub(crate) multi_store: Arc<QdrantMultiStore>,
    pub(crate) recent_cache: Option<Arc<RecentCache>>,
}

impl MemoryCoreService {
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        recent_cache: Option<Arc<RecentCache>>,
    ) -> Self {
        Self {
            sqlite_store,
            multi_store,
            recent_cache,
        }
    }

    // Basic CRUD operations - move existing logic here
    pub async fn save_entry(&self, entry: &MemoryEntry) -> Result<i64> {
        let saved_entry = self.sqlite_store.save(entry).await?;
        Ok(saved_entry.id.unwrap())
    }

    pub async fn load_recent(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        // Try cache first if available
        if let Some(cache) = &self.recent_cache {
            if let Some(cached_entries) = cache.get_recent(session_id, limit).await {
                return Ok(cached_entries);
            }
        }
        
        // Fallback to SQLite
        self.sqlite_store.load_recent(session_id, limit as i32).await
    }

    pub async fn store_analysis(&self, entry_id: i64, analysis: &UnifiedAnalysis) -> Result<()> {
        // Convert UnifiedAnalysis to SQLite format and store
        // Move existing analysis storage logic here
        todo!("Move from existing service.rs")
    }
}
