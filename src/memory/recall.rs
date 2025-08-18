// src/memory/recall.rs

//! Context-building strategies for memory recall.
//! Pulls recent session chat, fetches relevant semantic memories, applies decay.

use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;
use crate::memory::decay::{calculate_decayed_salience, DecayConfig};
use chrono::Utc;

/// The context returned for LLM prompting: recency + semantic + summaries.
#[derive(Default)]
pub struct RecallContext {
    pub recent: Vec<MemoryEntry>,   // Last N chronological messages (SQLite)
    pub semantic: Vec<MemoryEntry>, // Top K semantically relevant (Qdrant)
}

impl RecallContext {
    pub fn new(recent: Vec<MemoryEntry>, semantic: Vec<MemoryEntry>) -> Self {
        Self { recent, semantic }
    }
    
    /// Apply decay to all memories in context
    pub fn apply_decay(&mut self, config: &DecayConfig) {
        let now = Utc::now();
        
        // Apply decay to semantic memories (recent are too fresh to decay much)
        for memory in &mut self.semantic {
            let decayed = calculate_decayed_salience(memory, config, now);
            memory.salience = Some(decayed);
        }
        
        // Sort semantic by decayed salience
        self.semantic.sort_by(|a, b| {
            b.salience.unwrap_or(0.0)
                .partial_cmp(&a.salience.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    
    /// Get only the most emotionally significant memories
    pub fn high_salience_memories(&self) -> Vec<&MemoryEntry> {
        self.semantic.iter()
            .filter(|m| m.salience.unwrap_or(0.0) >= 7.0)
            .collect()
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
        // Get extra memories to account for decay filtering
        qdrant_store
            .semantic_search(session_id, embedding, semantic_count * 2)
            .await?
    } else {
        Vec::new()
    };

    // 3. Create context and apply decay
    let mut context = RecallContext::new(recent, semantic);
    let decay_config = DecayConfig::default();
    context.apply_decay(&decay_config);
    
    // 4. Trim to requested count after decay
    context.semantic.truncate(semantic_count);

    Ok(context)
}
