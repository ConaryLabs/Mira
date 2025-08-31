// src/services/chat/context.rs

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn, debug};

use crate::llm::client::OpenAIClient;
use crate::memory::recall::{RecallContext, build_context};
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

#[derive(Clone)]
pub struct ContextBuilder {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
    qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
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

    /// ── Phase 4: Enhanced context building with rolling summaries ──
    /// This is the main context building method that chooses between legacy and rolling approaches.
    pub async fn build_context(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        info!(
            "Building context for session: {} (history_cap={}, vector_results={}, rolling_summaries={})",
            session_id,
            self.config.history_message_cap(),
            self.config.max_vector_search_results(),
            CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100
        );

        // Check if we should use rolling summaries in context building
        if CONFIG.is_robust_memory_enabled() && 
           (CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100) {
            self.build_context_with_rolling_summaries(session_id, user_text).await
        } else {
            self.build_context_legacy(session_id, user_text).await
        }
    }

    /// ── Phase 4: Context building with rolling summaries integration ──
    async fn build_context_with_rolling_summaries(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        debug!("Building context with rolling summaries for session: {}", session_id);

        // Get embedding for semantic search
        let embedding = match self.llm_client.get_embedding(user_text).await {
            Ok(emb) => Some(emb),
            Err(e) => {
                warn!("Failed to get embedding for context building: {}", e);
                None
            }
        };

        // Load all recent messages (including summaries)
        let all_recent = self.sqlite_store
            .load_recent(session_id, self.config.history_message_cap() * 2)
            .await?;

        // Separate rolling summaries from regular messages
        let (rolling_summaries, regular_messages) = self.separate_summaries_from_messages(all_recent);

        // Build context strategically with rolling summaries
        let mut context_recent = Vec::new();
        
        // Strategy: Include the most recent actual messages for immediate context
        let immediate_context_size = std::cmp::min(5, self.config.history_message_cap() / 4);
        let recent_actual: Vec<MemoryEntry> = regular_messages
            .into_iter()
            .take(immediate_context_size)
            .collect();
        
        context_recent.extend(recent_actual);

        // Add rolling summaries for compressed historical context
        let (rolling_10, rolling_100) = self.select_relevant_rolling_summaries(rolling_summaries);
        
        // Include the latest 100-message summary first (broader context)
        if let Some(summary_100) = rolling_100 {
            debug!("Including 100-message rolling summary in context");
            context_recent.push(summary_100);
        }
        
        // Include the latest 10-message summary (more recent context)
        if let Some(summary_10) = rolling_10 {
            debug!("Including 10-message rolling summary in context");
            context_recent.push(summary_10);
        }

        // Perform semantic search with the user query
        let semantic_matches = if let Some(embedding) = embedding {
            match self.qdrant_store.semantic_search(session_id, &embedding, self.config.max_vector_search_results()).await {
                Ok(matches) => {
                    debug!("Found {} semantic matches from vector search", matches.len());
                    matches
                }
                Err(e) => {
                    warn!("Semantic search failed: {}", e);
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
        // Try the main context building method first
        match self.build_context(session_id, user_text).await {
            Ok(context) => Ok(context),
            Err(e) => {
                warn!("Main context building failed: {}. Trying minimal context.", e);
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
