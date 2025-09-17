// src/memory/recall/recall.rs
// Simplified context building - no decay calculation needed
// Just reads the current salience from database (already decayed by scheduler)

use crate::memory::core::traits::MemoryStore;
use crate::memory::core::types::MemoryEntry;
use tracing::info;

/// The context returned for LLM prompting
#[derive(Debug, Clone, Default)]
pub struct RecallContext {
    pub recent: Vec<MemoryEntry>,   // Last N chronological messages (SQLite)
    pub semantic: Vec<MemoryEntry>, // Top K semantically relevant (Qdrant)
}

impl RecallContext {
    pub fn new(recent: Vec<MemoryEntry>, semantic: Vec<MemoryEntry>) -> Self {
        Self { recent, semantic }
    }
    
    /// Get only high-salience memories (no calculation needed - DB is truth)
    pub fn high_salience_memories(&self) -> Vec<&MemoryEntry> {
        self.semantic.iter()
            .filter(|m| m.salience.unwrap_or(0.0) >= 7.0)
            .collect()
    }
    
    /// Filter to memories above a salience threshold
    pub fn filter_by_salience(&mut self, min_salience: f32) {
        self.semantic.retain(|m| m.salience.unwrap_or(0.0) >= min_salience);
    }
    
    /// Sort semantic memories by salience (highest first)
    pub fn sort_by_salience(&mut self) {
        self.semantic.sort_by(|a, b| {
            b.salience.unwrap_or(0.0)
                .partial_cmp(&a.salience.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

/// Build prompt context for a new message
/// Simple version - just reads from stores, no decay calculation
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
    // 1. Pull most recent chat history from SQLite
    let recent = sqlite_store
        .load_recent(session_id, recent_count)
        .await?;
    
    // 2. Pull semantically similar memories from Qdrant
    let semantic = if let Some(embedding) = user_embedding {
        // Get extra to account for filtering
        let search_count = (semantic_count as f32 * 1.5) as usize;
        qdrant_store
            .semantic_search(session_id, embedding, search_count)
            .await?
    } else {
        Vec::new()
    };
    
    // 3. Create context - salience values are already current in DB
    let mut context = RecallContext::new(recent, semantic);
    
    // 4. Sort by salience and filter weak memories
    context.sort_by_salience();
    context.filter_by_salience(3.0); // Keep memories with 30%+ strength
    context.semantic.truncate(semantic_count);
    
    info!(
        "Built context: {} recent, {} semantic memories",
        context.recent.len(),
        context.semantic.len()
    );
    
    Ok(context)
}
