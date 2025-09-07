// src/memory/parallel_recall.rs
// Enhanced parallel context building with multi-head support
// Uses superhuman stepped decay for optimal memory retention

use tokio::join;
use tracing::{debug, info, warn};
use std::collections::HashMap;
use crate::memory::recall::RecallContext;
use crate::memory::decay::{calculate_decayed_salience, should_include_memory, DecayConfig};
use crate::memory::traits::MemoryStore;
use crate::memory::qdrant::multi_store::QdrantMultiStore;
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;
use crate::config::CONFIG;
use chrono::Utc;

/// Enhanced memory entry with similarity score for re-ranking
#[derive(Debug, Clone)]
pub struct ScoredMemoryEntry {
    pub entry: crate::memory::types::MemoryEntry,
    pub similarity_score: f32,
    pub salience_score: f32,
    pub recency_score: f32,
    pub composite_score: f32,
    pub source_head: EmbeddingHead,
}

/// Build context with parallel operations for better performance
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
    
    // Launch embedding and recent messages fetch in parallel
    let (embedding_result, recent_result): (Result<Vec<f32>, _>, Result<Vec<_>, _>) = join!(
        llm_client.get_embedding(user_text),
        sqlite_store.load_recent(session_id, recent_count)
    );
    
    let parallel_time = start.elapsed();
    debug!("Parallel fetch completed in {:?} (embedding + SQLite)", parallel_time);
    
    // Handle results
    let embedding = embedding_result.ok();
    if embedding.is_none() {
        warn!("Failed to get embedding for user text");
    }
    
    let recent = recent_result?;
    debug!("Loaded {} recent messages", recent.len());
    
    // Semantic search only if we have an embedding
    let semantic = if let Some(ref emb) = embedding {
        let semantic_start = std::time::Instant::now();
        // Get extra results to account for filtering
        let search_count = (semantic_count as f32 * 1.5) as usize;
        let results = qdrant_store.semantic_search(session_id, emb, search_count).await?;
        debug!("Semantic search found {} results in {:?}", results.len(), semantic_start.elapsed());
        results
    } else {
        debug!("Skipping semantic search - no embedding available");
        Vec::new()
    };
    
    // Apply stepped decay and filtering
    let mut context = RecallContext::new(recent, semantic);
    let decay_config = DecayConfig::default();
    
    // Apply decay to semantic memories with the new stepped system
    let now = Utc::now();
    let mut filtered_semantic = Vec::new();
    
    for (idx, mut memory) in context.semantic.into_iter().enumerate() {
        let decayed = calculate_decayed_salience(&memory, &decay_config, now);
        memory.salience = Some(decayed);
        
        // Calculate relevance from position in search results
        let relevance = 1.0 - (idx as f32 / (semantic_count as f32 * 1.5));
        
        // Use the new filtering logic
        if should_include_memory(&memory, decayed, Some(relevance)) {
            filtered_semantic.push(memory);
        } else {
            debug!("Filtered out memory with salience {} and relevance {}", decayed, relevance);
        }
    }
    
    // Sort by composite score and trim
    filtered_semantic.sort_by(|a, b| {
        b.salience.unwrap_or(0.0)
            .partial_cmp(&a.salience.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    filtered_semantic.truncate(semantic_count);
    
    context.semantic = filtered_semantic;
    
    let total_time = start.elapsed();
    info!(
        "Context built in {:?}: {} recent, {} semantic entries", 
        total_time, context.recent.len(), context.semantic.len()
    );
    
    // Log performance metrics for tuning
    if total_time.as_millis() > 1000 {
        warn!("Slow context build: {:?} (consider optimization)", total_time);
    }
    
    Ok(context)
}

/// Enhanced multi-head parallel context building
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
    
    info!("Starting enhanced multi-head parallel context build for session: {}", session_id);
    
    // Parallel execution - embedding + recent messages + rolling summaries
    let (embedding_result, recent_result) = join!(
        llm_client.get_embedding(user_text),
        load_recent_with_summaries(sqlite_store, session_id, recent_count)
    );
    
    let embedding = embedding_result.map_err(|e| {
        warn!("Failed to get embedding for multi-head context building: {}", e);
        e
    })?;

    let context_recent = recent_result?;
    debug!("Loaded {} recent messages (including summaries) in parallel", context_recent.len());

    // Multi-head semantic search with appropriate k per head
    let k_per_head = std::cmp::max(10, semantic_count / 3);
    let multi_search_result = multi_store.search_all(session_id, &embedding, k_per_head).await?;

    debug!("Multi-head search completed: {} heads searched", multi_search_result.len());

    // Merge, deduplicate, and re-rank results
    let all_candidates = merge_and_deduplicate_results_vec(multi_search_result)?;
    let scored_entries = compute_rerank_scores(&embedding, all_candidates).await?;
    
    // Apply stepped decay to scored entries
    let now = Utc::now();
    let decay_config = DecayConfig::default();
    
    let mut final_entries: Vec<crate::memory::types::MemoryEntry> = scored_entries
        .into_iter()
        .filter_map(|mut scored| {
            let decayed = calculate_decayed_salience(&scored.entry, &decay_config, now);
            scored.entry.salience = Some(decayed);
            
            // Filter using new logic
            if should_include_memory(&scored.entry, decayed, Some(scored.similarity_score)) {
                Some(scored.entry)
            } else {
                None
            }
        })
        .collect();
    
    // Sort by salience and take top results
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
        "Enhanced multi-head context built in {:?}: {} recent messages, {} re-ranked semantic matches",
        total_time,
        context.recent.len(),
        context.semantic.len()
    );

    // Performance warning for slow builds
    if total_time.as_millis() > 1500 {
        warn!("Slow multi-head context build: {:?} (consider optimization)", total_time);
    }

    Ok(context)
}

/// Load recent messages with rolling summaries support
async fn load_recent_with_summaries<M>(
    sqlite_store: &M,
    session_id: &str,
    recent_count: usize,
) -> anyhow::Result<Vec<crate::memory::types::MemoryEntry>>
where
    M: MemoryStore + ?Sized,
{
    if CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100 {
        let all_recent = sqlite_store.load_recent(session_id, recent_count * 2).await?;
        
        // Filter to include summaries and recent messages
        let mut selected = Vec::new();
        let mut message_count = 0;
        
        for entry in all_recent {
            // Always include summaries
            if entry.memory_type == Some(crate::memory::types::MemoryType::Other) 
                && entry.tags.as_ref().map_or(false, |t| t.contains(&"summary".to_string())) {
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

/// Merge and deduplicate results from multiple heads (Vec version)
fn merge_and_deduplicate_results_vec(
    multi_results: Vec<(EmbeddingHead, Vec<crate::memory::types::MemoryEntry>)>
) -> anyhow::Result<Vec<ScoredMemoryEntry>> {
    let mut seen_ids = std::collections::HashSet::new();
    let mut scored_entries = Vec::new();
    
    for (head, entries) in multi_results {
        for (idx, entry) in entries.into_iter().enumerate() {
            let id = entry.id.clone().unwrap_or_default();
            if !seen_ids.contains(&id) {
                seen_ids.insert(id.clone());
                
                // Calculate similarity score from position
                let similarity = 1.0 - (idx as f32 / 100.0);
                
                scored_entries.push(ScoredMemoryEntry {
                    entry,
                    similarity_score: similarity,
                    salience_score: 0.0,  // Will be calculated later
                    recency_score: 0.0,   // Will be calculated later
                    composite_score: 0.0, // Will be calculated later
                    source_head: head.clone(),
                });
            }
        }
    }
    
    Ok(scored_entries)
}

/// Merge and deduplicate results from multiple heads (HashMap version - kept for compatibility)
fn merge_and_deduplicate_results(
    multi_results: HashMap<EmbeddingHead, Vec<crate::memory::types::MemoryEntry>>
) -> anyhow::Result<Vec<ScoredMemoryEntry>> {
    let mut seen_ids = std::collections::HashSet::new();
    let mut scored_entries = Vec::new();
    
    for (head, entries) in multi_results {
        for (idx, entry) in entries.into_iter().enumerate() {
            let id = entry.id.clone().unwrap_or_default();
            if !seen_ids.contains(&id) {
                seen_ids.insert(id.clone());
                
                // Calculate similarity score from position
                let similarity = 1.0 - (idx as f32 / 100.0);
                
                scored_entries.push(ScoredMemoryEntry {
                    entry,
                    similarity_score: similarity,
                    salience_score: 0.0,  // Will be calculated later
                    recency_score: 0.0,   // Will be calculated later
                    composite_score: 0.0, // Will be calculated later
                    source_head: head.clone(),
                });
            }
        }
    }
    
    Ok(scored_entries)
}

/// Compute re-ranking scores for candidates
async fn compute_rerank_scores(
    _query_embedding: &[f32],
    candidates: Vec<ScoredMemoryEntry>,
) -> anyhow::Result<Vec<ScoredMemoryEntry>> {
    let now = Utc::now();
    
    let mut reranked = Vec::new();
    for mut entry in candidates {
        // Calculate recency score (exponential decay over days)
        let age_days = now.signed_duration_since(entry.entry.timestamp).num_days() as f32;
        entry.recency_score = (-age_days / 30.0).exp(); // Half-life of 30 days
        
        // Salience score is the decayed salience
        entry.salience_score = entry.entry.salience.unwrap_or(5.0) / 10.0;
        
        // Composite score weights: similarity (40%), salience (40%), recency (20%)
        entry.composite_score = 
            entry.similarity_score * 0.4 +
            entry.salience_score * 0.4 +
            entry.recency_score * 0.2;
        
        reranked.push(entry);
    }
    
    Ok(reranked)
}

/// Adaptive context building that chooses the best method
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
    // Always use multi-head if available
    if let Some(multi_store) = multi_store {
        debug!("Using enhanced multi-head parallel context building");
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

    // Fallback to standard parallel recall
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

/// Performance metrics for monitoring
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

/// Enhanced context building with metrics collection
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
    
    // Build context using adaptive method
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

    // Collect metrics
    let metrics = ParallelRecallMetrics {
        session_id: session_id.to_string(),
        total_time_ms: total_time.as_millis() as u64,
        embedding_time_ms: 0, // Would need more instrumentation
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
