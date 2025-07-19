// src/memory/recall.rs

//! Context-building strategies for memory recall.
//! Pulls recent session chat, fetches relevant semantic memories, combines for LLM prompts.

use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;

/// The context returned for LLM prompting: recency + semantic + summaries.
pub struct RecallContext {
    pub recent: Vec<MemoryEntry>,   // Last N chronological messages (SQLite)
    pub semantic: Vec<MemoryEntry>, // Top K semantically relevant (Qdrant)
}

impl RecallContext {
    pub fn new(recent: Vec<MemoryEntry>, semantic: Vec<MemoryEntry>) -> Self {
        Self { recent, semantic }
    }
}

/// Strategy: build prompt context for a new message.
/// Loads last `recent_count` from SQLite and top `semantic_count` from Qdrant.
pub async fn build_context<M1, M2>(
    session_id: &str,
    user_embedding: Option<&[f32]>,
    recent_count: usize,
    semantic_count: usize,
    sqlite_store: &M1,
    qdrant_store: &M2,
) -> anyhow::Result<RecallContext>
where
    M1: MemoryStore + ?Sized,
    M2: MemoryStore + ?Sized,
{
    // 1. Pull most recent chat history from SQLite.
    let recent = sqlite_store
        .load_recent(session_id, recent_count)
        .await?;

    // 2. Pull top K semantically similar from Qdrant (if embedding provided).
    let semantic = if let Some(embedding) = user_embedding {
        qdrant_store
            .semantic_search(session_id, embedding, semantic_count)
            .await?
    } else {
        Vec::new()
    };

    Ok(RecallContext::new(recent, semantic))
}
