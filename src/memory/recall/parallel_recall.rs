// src/memory/recall/parallel_recall.rs
// Parallel recall with multi-head search - no decay calculations
// Database salience values are already current from scheduled decay

use tokio::join;
use tracing::{debug, info, warn};
use std::collections::HashSet;
use crate::memory::recall::recall::RecallContext;
use crate::memory::core::traits::MemoryStore;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;
use crate::config::CONFIG;
use chrono::Utc;

#[derive(Debug, Clone)]
pub struct ScoredMemoryEntry {
    pub entry: crate::memory::core::types::MemoryEntry,
    pub similarity_score: f32,
    pub salience_score: f32,
    pub recency_score: f32,
    pub composite_score: f32,
    pub source_head: EmbeddingHead,
}

/// Build context with parallel fetching - no decay needed
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
    
    debug!("Starting parallel context build for session: {}", session_id);
    
    // Parallel fetch embedding and recent messages
    let (embedding_result, recent_result): (Result<Vec<f32>, _>, Result<Vec<_>, _>) = join!(
        llm_client.get_embedding(user_text),
        sqlite_store.load_recent(session_id, recent_count)
    );
    
    let parallel_time = start.elapsed();
    debug!("Parallel fetch completed in {:?}", parallel_time);
    
    let embedding = embedding_result.ok();
    if embedding.is_none() {
        warn!("Failed to get embedding for user text");
    }
    
    let recent = recent_result?;
    debug!("Loaded {} recent messages", recent.len());
    
    // Semantic search if we have an embedding
    let mut semantic = if let Some(ref emb) = embedding {
        let semantic_start = std::time::Instant::now();
        let search_count = (semantic_count as f32 * 1.5) as usize;
        let results = qdrant_store.semantic_search(session_id, emb, search_count).await?;
        debug!("Semantic search found {} results in {:?}", results.len(), semantic_start.elapsed());
        results
    } else {
        Vec::new()
    };
    
    // Filter by salience threshold and sort
    // Salience is already decayed in DB - just use it directly
    semantic.retain(|m| m.salience.unwrap_or(0.0) >= 3.0);
    semantic.sort_by(|a, b| {
        b.salience.unwrap_or(0.0)
            .partial_cmp(&a.salience.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    semantic.truncate(semantic_count);
    
    let context = RecallContext::new(recent, semantic);
    
    let total_time = start.elapsed();
    info!(
        "Context built in {:?}: {} recent, {} semantic entries", 
        total_time, context.recent.len(), context.semantic.len()
    );
    
    if total_time.as_millis() > 1000 {
        warn!("Slow context build: {:?} (consider optimization)", total_time);
    }
    
    Ok(context)
}

/// Build context using multi-head search
pub async fn build_context_multi_head<M1>(
    session_id: &str,
    user_text: &str,
    recent_count: usize,
    semantic_count: usize,
    llm_client: &OpenAIClient,
    sqlite_store: &M1,
    multi_store: &QdrantMultiStore,
) -> anyhow::Result<RecallContext>
where
    M1: MemoryStore + ?Sized,
{
    let start = std::time::Instant::now();
    
    info!("Starting multi-head parallel context build for session: {}", session_id);
    
    // Parallel fetch embedding and recent messages
    let (embedding_result, recent_result) = join!(
        llm_client.get_embedding(user_text),
        load_recent_with_summaries(sqlite_store, session_id, recent_count)
    );
    
    let embedding = embedding_result.map_err(|e| {
        warn!("Failed to get embedding: {}", e);
        e
    })?;

    let context_recent = recent_result?;
    debug!("Loaded {} recent messages (including summaries)", context_recent.len());

    // Search across all heads
    let k_per_head = std::cmp::max(10, semantic_count / 3);
    let multi_search_result = multi_store.search_all(session_id, &embedding, k_per_head).await?;
    debug!("Multi-head search completed: {} heads searched", multi_search_result.len());

    // Merge and deduplicate
    let all_candidates = merge_and_deduplicate_results(multi_search_result)?;
    let scored_entries = compute_rerank_scores(&embedding, all_candidates).await?;
    
    // Filter and sort by composite score
    let mut final_entries: Vec<crate::memory::core::types::MemoryEntry> = scored_entries
        .into_iter()
        .filter(|scored| {
            // Use DB salience (already decayed) as filter
            scored.entry.salience.unwrap_or(0.0) >= 3.0
        })
        .map(|scored| scored.entry)
        .collect();
    
    // Sort by salience (DB values are current)
    final_entries.sort_by(|a, b| {
        b.salience.unwrap_or(0.0)
            .partial_cmp(&a.salience.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    final_entries.truncate(semantic_count);

    let context = RecallContext {
        recent: context_recent,
        semantic: final_entries,
    };

    let total_time = start.elapsed();
    info!(
        "Multi-head context built in {:?}: {} recent, {} semantic",
        total_time, context.recent.len(), context.semantic.len()
    );

    if total_time.as_millis() > 1500 {
        warn!("Slow multi-head context build: {:?}", total_time);
    }

    Ok(context)
}

/// Load recent messages including summaries if enabled
async fn load_recent_with_summaries<M>(
    sqlite_store: &M,
    session_id: &str,
    recent_count: usize,
) -> anyhow::Result<Vec<crate::memory::core::types::MemoryEntry>>
where
    M: MemoryStore + ?Sized,
{
    if CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100 {
        let all_recent = sqlite_store.load_recent(session_id, recent_count * 2).await?;
        
        let mut selected = Vec::new();
        let mut message_count = 0;
        
        for entry in all_recent {
            // Check if it's a summary using tags instead of memory_type
            let is_summary = entry.tags.as_ref()
                .map(|tags| tags.iter().any(|t| t.contains("summary")))
                .unwrap_or(false);
                
            if is_summary {
                selected.push(entry);
            } else if message_count < recent_count {
                selected.push(entry);
                message_count += 1;
            }
        }
        
        Ok(selected)
    } else {
        sqlite_store.load_recent(session_id, recent_count).await
    }
}

/// Merge results from multiple heads and deduplicate
fn merge_and_deduplicate_results(
    multi_results: Vec<(EmbeddingHead, Vec<crate::memory::core::types::MemoryEntry>)>
) -> anyhow::Result<Vec<ScoredMemoryEntry>> {
    let mut seen_ids = HashSet::new();
    let mut scored_entries = Vec::new();
    
    for (head, entries) in multi_results {
        for (idx, entry) in entries.into_iter().enumerate() {
            let id = entry.id.unwrap_or_default();
            if !seen_ids.contains(&id) {
                seen_ids.insert(id);
                
                let similarity = 1.0 - (idx as f32 / 100.0);
                
                scored_entries.push(ScoredMemoryEntry {
                    entry,
                    similarity_score: similarity,
                    salience_score: 0.0,
                    recency_score: 0.0,
                    composite_score: 0.0,
                    source_head: head,
                });
            }
        }
    }
    
    Ok(scored_entries)
}

/// Compute reranking scores for candidates
async fn compute_rerank_scores(
    _query_embedding: &[f32],
    candidates: Vec<ScoredMemoryEntry>,
) -> anyhow::Result<Vec<ScoredMemoryEntry>> {
    let now = Utc::now();
    
    let mut reranked = Vec::new();
    for mut entry in candidates {
        // Recency score based on age
        let age_days = now.signed_duration_since(entry.entry.timestamp).num_days() as f32;
        entry.recency_score = (-age_days / 30.0).exp();
        
        // Use DB salience directly (already decayed)
        entry.salience_score = entry.entry.salience.unwrap_or(5.0) / 10.0;
        
        // Composite score
        entry.composite_score = 
            entry.similarity_score * 0.4 +
            entry.salience_score * 0.4 +
            entry.recency_score * 0.2;
        
        reranked.push(entry);
    }
    
    Ok(reranked)
}

/// Adaptive context building - uses multi-head if available
pub async fn build_context_adaptive<M1, M2>(
    session_id: &str,
    user_text: &str,
    recent_count: usize,
    semantic_count: usize,
    llm_client: &OpenAIClient,
    sqlite_store: &M1,
    qdrant_store: &M2,
    multi_store: Option<&QdrantMultiStore>,
) -> anyhow::Result<RecallContext>
where
    M1: MemoryStore + ?Sized,
    M2: MemoryStore + ?Sized,
{
    if let Some(multi_store) = multi_store {
        debug!("Using multi-head parallel context building");
        return build_context_multi_head(
            session_id,
            user_text,
            recent_count,
            semantic_count,
            llm_client,
            sqlite_store,
            multi_store,
        ).await;
    }

    build_context_parallel(
        session_id,
        user_text,
        recent_count,
        semantic_count,
        llm_client,
        sqlite_store,
        qdrant_store,
    ).await
}

#[derive(Debug, Clone)]
pub struct ParallelRecallMetrics {
    pub session_id: String,
    pub total_time_ms: u64,
    pub embedding_time_ms: u64,
    pub search_time_ms: u64,
    pub rerank_time_ms: u64,
    pub recent_count: usize,
    pub semantic_count: usize,
    pub candidates_before_rerank: usize,
    pub multi_head_enabled: bool,
    pub heads_searched: usize,
}

/// Build context with metrics tracking
pub async fn build_context_with_metrics<M1, M2>(
    session_id: &str,
    user_text: &str,
    recent_count: usize,
    semantic_count: usize,
    llm_client: &OpenAIClient,
    sqlite_store: &M1,
    qdrant_store: &M2,
    multi_store: Option<&QdrantMultiStore>,
) -> anyhow::Result<(RecallContext, ParallelRecallMetrics)>
where
    M1: MemoryStore + ?Sized,
    M2: MemoryStore + ?Sized,
{
    let total_start = std::time::Instant::now();
    
    let context = build_context_adaptive(
        session_id,
        user_text,
        recent_count,
        semantic_count,
        llm_client,
        sqlite_store,
        qdrant_store,
        multi_store,
    ).await?;

    let total_time = total_start.elapsed();

    let metrics = ParallelRecallMetrics {
        session_id: session_id.to_string(),
        total_time_ms: total_time.as_millis() as u64,
        embedding_time_ms: 0,
        search_time_ms: 0,
        rerank_time_ms: 0,
        recent_count: context.recent.len(),
        semantic_count: context.semantic.len(),
        candidates_before_rerank: 0,
        multi_head_enabled: multi_store.is_some(),
        heads_searched: if multi_store.is_some() { 3 } else { 1 },
    };

    Ok((context, metrics))
}
