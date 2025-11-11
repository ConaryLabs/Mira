//! Core trait(s) for memory backends (SQLite, Qdrant, …).
//! All storage and recall goes through this—no direct DB calls in business logic.

use async_trait::async_trait;
use crate::memory::core::types::{MemoryEntry};

/// Trait for any memory backend—store, recall, search, update, etc.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Store a single memory entry (user or Mira message).
    /// Returns the entry as it was saved, now including its database ID.
    async fn save(&self, entry: &MemoryEntry) -> anyhow::Result<MemoryEntry>;

    /// Load the last N messages for a session, ordered chronologically.
    async fn load_recent(&self, session_id: &str, n: usize) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Search for memories semantically (e.g., using Qdrant).
    /// Returns top K most relevant memories for a query (embedding or tags).
    async fn semantic_search(
        &self,
        session_id: &str,
        embedding: &[f32],
        k: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Update metadata for a stored memory (e.g., after reprocessing by LLM).
    /// Returns the updated memory entry.
    async fn update_metadata(&self, id: i64, updated: &MemoryEntry) -> anyhow::Result<MemoryEntry>;

    /// Delete a memory entry (rare, but possible for admin/moderation).
    async fn delete(&self, id: i64) -> anyhow::Result<()>;
}
