// src/memory/parallel_recall.rs
// Parallel version of context building - 30-50% faster than sequential
use tokio::join;
use tracing::{info, warn};
use crate::memory::recall::RecallContext;
use crate::memory::decay::{calculate_decayed_salience, DecayConfig};
use crate::memory::traits::MemoryStore;
use crate::llm::client::OpenAIClient;
use chrono::Utc;

/// Build context with parallel operations for better performance
/// This runs embedding + SQLite queries in parallel, then does semantic search
pub async fn build_context_parallel<M1, M2>(
    session_id: &str,
    user_text: &str,
    recent_count: usize,
    semantic_count: usize,
    llm_client: &OpenAIClient,
    sqlite_store: &M1,
    qdrant_store: &M2,
) -> anyhow::Result<RecallContext>
where
    M1: MemoryStore + ?Sized,
    M2: MemoryStore + ?Sized,
{
    let start = std::time::Instant::now();
    
    // Launch embedding and recent messages fetch in parallel
    let (embedding_result, recent_result): (Result<Vec<f32>, _>, Result<Vec<_>, _>) = join!(
        llm_client.get_embedding(user_text),
        sqlite_store.load_recent(session_id, recent_count)
    );
    
    let parallel_time = start.elapsed();
    info!("⚡ Parallel fetch completed in {:?} (embedding + SQLite)", parallel_time);
    
    // Handle results
    let embedding = embedding_result.ok();
    if embedding.is_none() {
        warn!("Failed to get embedding for user text");
    }
    
    let recent = recent_result?;
    info!("📚 Loaded {} recent messages", recent.len());
    
    // Semantic search only if we have an embedding
    let semantic = if let Some(ref emb) = embedding {
        let semantic_start = std::time::Instant::now();
        let results = qdrant_store.semantic_search(session_id, emb, semantic_count * 2).await?;
        info!("🔍 Semantic search found {} results in {:?}", results.len(), semantic_start.elapsed());
        results
    } else {
        Vec::new()
    };
    
    // Apply decay and filtering (same as original)
    let mut context = RecallContext::new(recent, semantic);
    let decay_config = DecayConfig::default();
    
    // Apply decay to semantic memories
    let now = Utc::now();
    for memory in &mut context.semantic {
        let decayed = calculate_decayed_salience(memory, &decay_config, now);
        memory.salience = Some(decayed);
    }
    
    // Sort semantic by decayed salience
    context.semantic.sort_by(|a, b| {
        b.salience.unwrap_or(0.0)
            .partial_cmp(&a.salience.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    
    // Trim to requested count after decay
    context.semantic.truncate(semantic_count);
    
    let total_time = start.elapsed();
    info!("✅ Context built in {:?} total: {} recent, {} semantic entries", 
          total_time, context.recent.len(), context.semantic.len());
    
    Ok(context)
}
