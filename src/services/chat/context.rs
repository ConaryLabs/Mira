// src/services/chat/context.rs

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn, debug};
use tokio::join;
use std::collections::HashMap;

use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::recall::{RecallContext, build_context};
use crate::memory::qdrant::multi_store::QdrantMultiStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;
use crate::services::chat::config::ChatConfig;
use crate::config::CONFIG;

#[derive(Debug)]
pub struct ContextStats {
    pub total_messages: usize,
    pub recent_messages: usize,
    pub semantic_matches: usize,
    pub rolling_summaries: usize,
    pub compression_ratio: f64,
}

/// Enhanced memory entry with similarity score for re-ranking
#[derive(Debug, Clone)]
pub struct ScoredMemoryEntry {
    pub entry: MemoryEntry,
    pub similarity_score: f32,
    pub salience_score: f32,
    pub recency_score: f32,
    pub composite_score: f32,
    pub source_head: EmbeddingHead,
}

#[derive(Clone)]
pub struct ContextBuilder {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
    qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
    multi_store: Option<Arc<QdrantMultiStore>>,
    config: ChatConfig,
}

impl ContextBuilder {
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
        config: ChatConfig,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            qdrant_store,
            multi_store: None,
            config,
        }
    }

    /// Phase 5: Constructor with multi-store support
    pub fn new_with_multi_store(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
        multi_store: Option<Arc<QdrantMultiStore>>,
        config: ChatConfig,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            qdrant_store,
            multi_store,
            config,
        }
    }

    pub fn sqlite_store(&self) -> &Arc<dyn MemoryStore + Send + Sync> {
        &self.sqlite_store
    }

    pub fn qdrant_store(&self) -> &Arc<dyn MemoryStore + Send + Sync> {
        &self.qdrant_store
    }

    pub fn config(&self) -> &ChatConfig {
        &self.config
    }

    pub fn can_use_vector_search(&self) -> bool {
        self.config.enable_vector_search()
    }

    /// ── Phase 5: Enhanced context building with multi-head parallel retrieval ──
    /// This is the main context building method that chooses between enhanced and legacy approaches.
    pub async fn build_context(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        info!(
            "Building context for session: {} (history_cap={}, vector_results={}, rolling_summaries={}, multi_head={})",
            session_id,
            self.config.history_message_cap(),
            self.config.max_vector_search_results(),
            CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100,
            CONFIG.is_robust_memory_enabled() && self.multi_store.is_some()
        );

        // Phase 5: Use enhanced multi-head parallel retrieval when enabled
        if CONFIG.is_robust_memory_enabled() && self.multi_store.is_some() {
            self.build_context_with_multihead_retrieval(session_id, user_text).await
        } else if CONFIG.is_robust_memory_enabled() && 
                  (CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100) {
            self.build_context_with_rolling_summaries(session_id, user_text).await
        } else {
            self.build_context_legacy(session_id, user_text).await
        }
    }

    /// ── Phase 5: Multi-head parallel retrieval with re-ranking ──
    async fn build_context_with_multihead_retrieval(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        debug!("Building context with multi-head parallel retrieval for session: {}", session_id);
        let start_time = std::time::Instant::now();

        let multi_store = self.multi_store.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Multi-store not available for enhanced retrieval"))?;

        // Phase 5: Parallel execution - embedding + recent messages + multi-head search
        let (embedding_result, recent_result) = join!(
            self.llm_client.get_embedding(user_text),
            self.load_recent_with_summaries(session_id)
        );

        let embedding = embedding_result.map_err(|e| {
            warn!("Failed to get embedding for enhanced context building: {}", e);
            e
        })?;

        let context_recent = recent_result?;
        debug!("Loaded {} recent messages in parallel", context_recent.len());

        // Phase 5: Parallel multi-head semantic search
        let k_per_head = std::cmp::max(10, self.config.max_vector_search_results() / 3);
        let multi_search_result = multi_store.search_all(session_id, &embedding, k_per_head).await?;

        let num_heads_searched = multi_search_result.len();
        debug!("Multi-head search completed: {} heads searched", num_heads_searched);

        // Phase 5: Merge and deduplicate results across all heads
        let mut all_candidates = Vec::new();
        let mut content_dedup = HashMap::new();

        for (head, entries) in &multi_search_result {
            for entry in entries {
                // Simple deduplication by content hash to avoid identical chunks
                let content_key = format!("{}:{}", entry.content.len(), entry.content.chars().take(50).collect::<String>());
                
                if !content_dedup.contains_key(&content_key) {
                    content_dedup.insert(content_key, true);
                    all_candidates.push((*head, entry.clone()));
                } else {
                    debug!("Deduplicated similar content from {} head", head.as_str());
                }
            }
        }

        debug!("After deduplication: {} candidates from {} heads", all_candidates.len(), num_heads_searched);

        // Phase 5: Compute re-ranking scores for all candidates
        let scored_entries = self.compute_rerank_scores(&embedding, all_candidates).await?;

        // Phase 5: Sort by composite score and take top results
        let mut sorted_entries = scored_entries;
        sorted_entries.sort_by(|a, b| b.composite_score.partial_cmp(&a.composite_score).unwrap_or(std::cmp::Ordering::Equal));
        
        let selected_entries: Vec<MemoryEntry> = sorted_entries
            .into_iter()
            .take(self.config.max_vector_search_results())
            .map(|scored| scored.entry)
            .collect();

        let context = RecallContext {
            recent: context_recent,
            semantic: selected_entries,
        };

        let total_time = start_time.elapsed();
        info!(
            "Enhanced multi-head context built in {:?}: {} recent messages, {} re-ranked semantic matches from parallel search",
            total_time,
            context.recent.len(),
            context.semantic.len()
        );

        // Log performance warning if slow
        if total_time.as_millis() > 1500 {
            warn!("Slow multi-head context build: {:?} (consider optimization)", total_time);
        }

        Ok(context)
    }

    /// Phase 5: Compute composite re-ranking scores combining similarity, salience, and recency
    async fn compute_rerank_scores(
        &self,
        query_embedding: &[f32],
        candidates: Vec<(EmbeddingHead, MemoryEntry)>,
    ) -> Result<Vec<ScoredMemoryEntry>> {
        let mut scored_entries = Vec::new();
        let now = chrono::Utc::now();

        for (head, entry) in candidates {
            // Calculate similarity score
            let similarity_score = if let Some(entry_embedding) = &entry.embedding {
                self.cosine_similarity(query_embedding, entry_embedding)
            } else {
                0.0 // No embedding available
            };

            // Calculate salience score (normalize to 0-1 range)
            let salience_score = entry.salience.unwrap_or(0.0).min(1.0).max(0.0);

            // Calculate recency score (exponential decay from timestamp)
            let hours_ago = (now - entry.timestamp).num_hours().max(0) as f32;
            let recency_score = (-hours_ago / 168.0).exp(); // 168 hours = 1 week half-life

            // Phase 5: Composite score - 75% similarity + 20% salience + 5% recency
            // Adjust weights based on head type
            let (sim_weight, sal_weight, rec_weight) = match head {
                EmbeddingHead::Code => (0.70, 0.25, 0.05),    // Slightly favor salience for code
                EmbeddingHead::Summary => (0.80, 0.15, 0.05), // Favor similarity for summaries  
                EmbeddingHead::Semantic => (0.75, 0.20, 0.05), // Balanced default
            };

            let composite_score = (similarity_score * sim_weight) + 
                                (salience_score * sal_weight) + 
                                (recency_score * rec_weight);

            scored_entries.push(ScoredMemoryEntry {
                entry,
                similarity_score,
                salience_score,
                recency_score,
                composite_score,
                source_head: head,
            });
        }

        debug!("Computed re-ranking scores for {} candidates", scored_entries.len());
        Ok(scored_entries)
    }

    /// Phase 5: Cosine similarity calculation for embeddings
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot_product / (norm_a * norm_b)
        }
    }

    /// Phase 5: Load recent messages with rolling summaries integration
    async fn load_recent_with_summaries(&self, session_id: &str) -> Result<Vec<MemoryEntry>> {
        if CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100 {
            // Use rolling summaries approach
            let all_recent = self.sqlite_store
                .load_recent(session_id, self.config.history_message_cap() * 2)
                .await?;

            let (rolling_summaries, regular_messages) = self.separate_summaries_from_messages(all_recent);
            let mut context_recent = Vec::new();

            // Include recent actual messages
            let immediate_context_size = std::cmp::min(8, self.config.history_message_cap() / 3);
            context_recent.extend(regular_messages.into_iter().take(immediate_context_size));

            // Add most relevant rolling summaries
            let (rolling_10, rolling_100) = self.select_relevant_rolling_summaries(rolling_summaries);
            if let Some(summary_100) = rolling_100 {
                context_recent.push(summary_100);
            }
            if let Some(summary_10) = rolling_10 {
                context_recent.push(summary_10);
            }

            Ok(context_recent)
        } else {
            // Load regular recent messages
            self.sqlite_store.load_recent(session_id, self.config.history_message_cap()).await
        }
    }

    /// ── Phase 4: Context building with rolling summaries integration ──
    async fn build_context_with_rolling_summaries(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        debug!("Building context with rolling summaries for session: {}", session_id);

        // Load recent messages and separate summaries from regular messages
        let all_recent = self.sqlite_store
            .load_recent(session_id, self.config.history_message_cap() * 2)
            .await?;

        let (rolling_summaries, regular_messages) = self.separate_summaries_from_messages(all_recent);
        
        // Build context with both summaries and recent messages
        let mut context_recent = Vec::new();
        
        // Include immediate context (recent actual messages)
        let immediate_context_size = std::cmp::min(5, self.config.history_message_cap() / 4);
        context_recent.extend(regular_messages.into_iter().take(immediate_context_size));

        // Select and include the most relevant rolling summaries
        let (rolling_10, rolling_100) = self.select_relevant_rolling_summaries(rolling_summaries);
        if let Some(summary_100) = rolling_100 {
            context_recent.push(summary_100);
        }
        if let Some(summary_10) = rolling_10 {
            context_recent.push(summary_10);
        }

        debug!("Context recent built: {} messages", context_recent.len());

        // Get semantic matches if vector search is enabled
        let semantic_matches = if self.can_use_vector_search() {
            match self.llm_client.get_embedding(user_text).await {
                Ok(embedding) => match self.qdrant_store.semantic_search(session_id, &embedding, self.config.max_vector_search_results()).await {
                    Ok(matches) => {
                        debug!("Found {} semantic matches from vector search", matches.len());
                        matches
                    }
                    Err(e) => {
                        warn!("Semantic search failed: {}", e);
                        Vec::new()
                    }
                }
                Err(e) => {
                    warn!("Failed to get embedding for context building: {}", e);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        let context = RecallContext {
            recent: context_recent,
            semantic: semantic_matches,
        };

        info!(
            "Context built with rolling summaries: {} recent messages ({} actual + {} summaries), {} semantic matches",
            context.recent.len(),
            context.recent.iter().filter(|m| !self.is_summary_message(m)).count(),
            context.recent.iter().filter(|m| self.is_summary_message(m)).count(),
            context.semantic.len()
        );

        Ok(context)
    }

    /// ── Legacy context building (when rolling summaries are disabled) ──
    async fn build_context_legacy(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        debug!("Building context using legacy method for session: {}", session_id);

        let embedding = match self.llm_client.get_embedding(user_text).await {
            Ok(emb) => Some(emb),
            Err(e) => {
                warn!("Failed to get embedding for context building: {}", e);
                None
            }
        };

        let context = build_context(
            session_id,
            embedding.as_deref(),
            self.config.history_message_cap(),
            self.config.max_vector_search_results(),
            self.sqlite_store.as_ref(),
            self.qdrant_store.as_ref(),
        )
        .await
        .unwrap_or_else(|e| {
            warn!("Failed to build full context: {}. Using empty context.", e);
            RecallContext {
                recent: Vec::new(),
                semantic: Vec::new(),
            }
        });

        info!(
            "Legacy context built: {} recent messages, {} semantic matches",
            context.recent.len(),
            context.semantic.len()
        );

        Ok(context)
    }

    /// ── Phase 4: Separate rolling summaries from regular messages ──
    fn separate_summaries_from_messages(
        &self,
        messages: Vec<MemoryEntry>,
    ) -> (Vec<MemoryEntry>, Vec<MemoryEntry>) {
        let mut rolling_summaries = Vec::new();
        let mut regular_messages = Vec::new();

        for message in messages {
            if self.is_rolling_summary(&message) {
                rolling_summaries.push(message);
            } else {
                regular_messages.push(message);
            }
        }

        debug!(
            "Separated {} rolling summaries from {} regular messages",
            rolling_summaries.len(),
            regular_messages.len()
        );

        (rolling_summaries, regular_messages)
    }

    /// ── Phase 4: Select the most relevant rolling summaries ──
    fn select_relevant_rolling_summaries(
        &self,
        summaries: Vec<MemoryEntry>,
    ) -> (Option<MemoryEntry>, Option<MemoryEntry>) {
        let mut latest_10: Option<MemoryEntry> = None;
        let mut latest_100: Option<MemoryEntry> = None;

        for summary in summaries {
            if let Some(tags) = &summary.tags {
                if tags.iter().any(|tag| tag == "summary:rolling:10") {
                    if latest_10.is_none() || summary.timestamp > latest_10.as_ref().unwrap().timestamp {
                        latest_10 = Some(summary);
                    }
                } else if tags.iter().any(|tag| tag == "summary:rolling:100") {
                    if latest_100.is_none() || summary.timestamp > latest_100.as_ref().unwrap().timestamp {
                        latest_100 = Some(summary);
                    }
                }
            }
        }

        debug!(
            "Selected rolling summaries: 10-msg={}, 100-msg={}",
            latest_10.is_some(),
            latest_100.is_some()
        );

        (latest_10, latest_100)
    }

    /// ── Phase 4: Check if a message is a rolling summary ──
    fn is_rolling_summary(&self, message: &MemoryEntry) -> bool {
        message.tags
            .as_ref()
            .map(|tags| {
                tags.iter().any(|tag| {
                    tag.starts_with("summary:rolling:")
                })
            })
            .unwrap_or(false)
    }

    /// ── Check if a message is any kind of summary ──
    fn is_summary_message(&self, message: &MemoryEntry) -> bool {
        message.tags
            .as_ref()
            .map(|tags| tags.iter().any(|tag| tag.contains("summary")))
            .unwrap_or(false)
    }

    /// ── Build context with fallbacks (used by ContextService) ──
    pub async fn build_context_with_fallbacks(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        // Try the enhanced context building method first
        match self.build_context(session_id, user_text).await {
            Ok(context) => Ok(context),
            Err(e) => {
                warn!("Enhanced context building failed: {}. Trying minimal context.", e);
                self.build_minimal_context(session_id).await
            }
        }
    }

    /// ── Build minimal context (fallback for when vector search fails) ──
    pub async fn build_minimal_context(&self, session_id: &str) -> Result<RecallContext> {
        info!("Building minimal context for session: {}", session_id);

        let recent = self.sqlite_store
            .load_recent(session_id, self.config.history_message_cap())
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to load recent messages: {}", e);
                Vec::new()
            });

        // When using minimal context with rolling summaries, still prefer summaries
        let context = if CONFIG.is_robust_memory_enabled() && 
                        (CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100) {
            let (summaries, regular) = self.separate_summaries_from_messages(recent);
            let (rolling_10, rolling_100) = self.select_relevant_rolling_summaries(summaries);
            
            let mut context_recent = Vec::new();
            
            // Include a few recent actual messages
            context_recent.extend(regular.into_iter().take(3));
            
            // Add summaries
            if let Some(summary_100) = rolling_100 {
                context_recent.push(summary_100);
            }
            if let Some(summary_10) = rolling_10 {
                context_recent.push(summary_10);
            }
            
            RecallContext {
                recent: context_recent,
                semantic: Vec::new(),
            }
        } else {
            RecallContext {
                recent,
                semantic: Vec::new(),
            }
        };

        info!("Minimal context built: {} recent messages", context.recent.len());
        Ok(context)
    }

    /// ── Get context statistics for monitoring and debugging ──
    pub async fn get_context_stats(&self, session_id: &str) -> Result<ContextStats> {
        let all_messages = self.sqlite_store
            .load_recent(session_id, 1000) // Load many to get accurate counts
            .await?;

        let (summaries, regular) = self.separate_summaries_from_messages(all_messages);
        let total_messages = summaries.len() + regular.len();
        let rolling_summaries = summaries.len();
        
        let compression_ratio = if total_messages > 0 {
            rolling_summaries as f64 / total_messages as f64
        } else {
            0.0
        };

        let stats = ContextStats {
            total_messages,
            recent_messages: std::cmp::min(regular.len(), self.config.history_message_cap()),
            semantic_matches: self.config.max_vector_search_results(),
            rolling_summaries,
            compression_ratio,
        };

        debug!("Context stats for session {}: {:?}", session_id, stats);
        Ok(stats)
    }

    /// ── Phase 4: Get rolling summary status for a session ──
    pub async fn get_rolling_summary_status(&self, session_id: &str) -> Result<RollingSummaryStatus> {
        let all_messages = self.sqlite_store
            .load_recent(session_id, 500)
            .await?;

        let (summaries, regular_messages) = self.separate_summaries_from_messages(all_messages);
        
        let mut rolling_10_count = 0;
        let mut rolling_100_count = 0;
        let mut latest_10_timestamp = None;
        let mut latest_100_timestamp = None;

        for summary in summaries {
            if let Some(tags) = &summary.tags {
                if tags.iter().any(|tag| tag == "summary:rolling:10") {
                    rolling_10_count += 1;
                    if latest_10_timestamp.is_none() || summary.timestamp > latest_10_timestamp.unwrap() {
                        latest_10_timestamp = Some(summary.timestamp);
                    }
                } else if tags.iter().any(|tag| tag == "summary:rolling:100") {
                    rolling_100_count += 1;
                    if latest_100_timestamp.is_none() || summary.timestamp > latest_100_timestamp.unwrap() {
                        latest_100_timestamp = Some(summary.timestamp);
                    }
                }
            }
        }

        let status = RollingSummaryStatus {
            session_id: session_id.to_string(),
            total_messages: regular_messages.len(),
            rolling_10_count,
            rolling_100_count,
            latest_10_timestamp,
            latest_100_timestamp,
            enabled: CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100,
        };

        Ok(status)
    }

    /// ── Build context for a specific embedding (used by ContextService) ──
    pub async fn build_context_with_embedding(
        &self,
        session_id: &str,
        embedding: &[f32],
    ) -> Result<RecallContext> {
        debug!("Building context with provided embedding for session: {}", session_id);

        if CONFIG.is_robust_memory_enabled() && 
           (CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100) {
            // Use rolling summaries approach
            let all_recent = self.sqlite_store
                .load_recent(session_id, self.config.history_message_cap() * 2)
                .await?;

            let (rolling_summaries, regular_messages) = self.separate_summaries_from_messages(all_recent);
            
            let mut context_recent = Vec::new();
            let immediate_context_size = std::cmp::min(5, self.config.history_message_cap() / 4);
            context_recent.extend(regular_messages.into_iter().take(immediate_context_size));

            let (rolling_10, rolling_100) = self.select_relevant_rolling_summaries(rolling_summaries);
            if let Some(summary_100) = rolling_100 {
                context_recent.push(summary_100);
            }
            if let Some(summary_10) = rolling_10 {
                context_recent.push(summary_10);
            }

            let semantic_matches = self.qdrant_store
                .semantic_search(session_id, embedding, self.config.max_vector_search_results())
                .await
                .unwrap_or_else(|e| {
                    warn!("Semantic search failed: {}", e);
                    Vec::new()
                });

            Ok(RecallContext {
                recent: context_recent,
                semantic: semantic_matches,
            })
        } else {
            // Use legacy approach
            build_context(
                session_id,
                Some(embedding),
                self.config.history_message_cap(),
                self.config.max_vector_search_results(),
                self.sqlite_store.as_ref(),
                self.qdrant_store.as_ref(),
            ).await
        }
    }
}

/// ── Phase 4: Rolling summary status for monitoring ──
#[derive(Debug, Clone)]
pub struct RollingSummaryStatus {
    pub session_id: String,
    pub total_messages: usize,
    pub rolling_10_count: usize,
    pub rolling_100_count: usize,
    pub latest_10_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub latest_100_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub enabled: bool,
}
