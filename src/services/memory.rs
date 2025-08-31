// src/services/memory.rs
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::CONFIG;
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::{EmbeddingHead, TextChunker};
use crate::memory::{
    qdrant::{multi_store::QdrantMultiStore, store::QdrantMemoryStore},
    sqlite::store::SqliteMemoryStore,
    traits::MemoryStore,
    types::{MemoryEntry, MemoryType},
    recall::RecallContext,
};
use crate::services::chat::ChatResponse;

/// ── Phase 5: Enhanced memory entry with similarity score for re-ranking ──
#[derive(Debug, Clone)]
pub struct ScoredMemoryEntry {
    pub entry: MemoryEntry,
    pub similarity_score: f32,
    pub salience_score: f32,
    pub recency_score: f32,
    pub composite_score: f32,
    pub source_head: EmbeddingHead,
}

pub struct MemoryService {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_store: Arc<QdrantMemoryStore>,
    multi_store: Option<Arc<QdrantMultiStore>>,

    // ── Phase 4: Session message counters for rolling summaries ──
    session_counters: Arc<RwLock<HashMap<String, usize>>>,
}

impl MemoryService {
    /// Create a new memory service with single-head and multi-head embedding support.
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            qdrant_store,
            multi_store: None,
            session_counters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// ── Phase 1: Constructor with multi-store support ──
    pub fn new_with_multi_store(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        info!("🏗️ MemoryService initialized with multi-collection support");
        Self {
            llm_client,
            sqlite_store,
            qdrant_store,
            multi_store: Some(multi_store),
            session_counters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// ── Phase 5: Parallel recall context method for enhanced retrieval ──
    /// This method encapsulates the multi-head parallel retrieval logic and provides
    /// a clean interface for ContextService and ChatService to use.
    pub async fn parallel_recall_context(
        &self,
        session_id: &str,
        query_text: &str,
        recent_count: usize,
        semantic_count: usize,
    ) -> Result<RecallContext> {
        info!("🔄 Starting parallel recall context for session: {} (robust_mode={})", 
              session_id, CONFIG.is_robust_memory_enabled());
        
        let _start_time = std::time::Instant::now();

        // Phase 5: Use enhanced multi-head retrieval if available
        if CONFIG.is_robust_memory_enabled() && self.multi_store.is_some() {
            self.build_context_with_multihead_parallel(session_id, query_text, recent_count, semantic_count).await
        } else {
            // Fallback to existing parallel recall from parallel_recall.rs
            let embedding_result = self.llm_client.get_embedding(query_text).await;
            match embedding_result {
                Ok(_embedding) => {
                    crate::memory::parallel_recall::build_context_parallel(
                        session_id,
                        query_text,
                        recent_count,
                        semantic_count,
                        &*self.llm_client,
                        self.sqlite_store.as_ref(),
                        self.qdrant_store.as_ref(),
                    ).await
                }
                Err(e) => {
                    warn!("Failed to get embedding for parallel recall: {}", e);
                    // Fallback to just recent messages
                    let recent = self.sqlite_store.load_recent(session_id, recent_count).await?;
                    Ok(RecallContext::new(recent, Vec::new()))
                }
            }
        }
    }

    /// ── Phase 5: Multi-head parallel context building implementation ──
    async fn build_context_with_multihead_parallel(
        &self,
        session_id: &str,
        query_text: &str,
        recent_count: usize,
        semantic_count: usize,
    ) -> Result<RecallContext> {
        debug!("Building context with multi-head parallel retrieval");
        
        let multi_store = self.multi_store.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Multi-store not available"))?;

        // Phase 5: Parallel execution - embedding + recent messages
        let (embedding_result, recent_result) = tokio::join!(
            self.llm_client.get_embedding(query_text),
            self.load_recent_with_rolling_summaries(session_id, recent_count)
        );

        let embedding = embedding_result?;
        let context_recent = recent_result?;
        debug!("Loaded {} recent messages in parallel", context_recent.len());

        // Phase 5: Multi-head semantic search with appropriate k per head
        let k_per_head = std::cmp::max(10, semantic_count / 3);
        let multi_search_result = multi_store.search_all(session_id, &embedding, k_per_head).await?;

        // Phase 5: Merge, deduplicate, and re-rank results
        let all_candidates = self.merge_and_deduplicate_results(multi_search_result)?;
        let scored_entries = self.compute_rerank_scores(&embedding, all_candidates).await?;
        
        // Sort by composite score and take top results
        let mut sorted_entries = scored_entries;
        sorted_entries.sort_by(|a, b| b.composite_score.partial_cmp(&a.composite_score)
                                 .unwrap_or(std::cmp::Ordering::Equal));
        
        let selected_entries: Vec<MemoryEntry> = sorted_entries
            .into_iter()
            .take(semantic_count)
            .map(|scored| scored.entry)
            .collect();

        let context = RecallContext {
            recent: context_recent,
            semantic: selected_entries,
        };

        info!(
            "Multi-head parallel context built: {} recent, {} re-ranked semantic matches",
            context.recent.len(),
            context.semantic.len()
        );

        Ok(context)
    }

    /// ── Phase 5: Load recent messages with rolling summaries support ──
    async fn load_recent_with_rolling_summaries(
        &self,
        session_id: &str,
        recent_count: usize,
    ) -> Result<Vec<MemoryEntry>> {
        if CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100 {
            let all_recent = self.sqlite_store.load_recent(session_id, recent_count * 2).await?;
            
            let (summaries, regular): (Vec<_>, Vec<_>) = all_recent.into_iter()
                .partition(|entry| self.is_rolling_summary_entry(entry));

            let mut context_recent = Vec::new();
            
            // Include recent actual messages
            let immediate_count = std::cmp::min(8, recent_count / 3);
            context_recent.extend(regular.into_iter().take(immediate_count));

            // Add most relevant rolling summaries
            if let (Some(summary_10), Some(summary_100)) = self.select_best_rolling_summaries(summaries) {
                context_recent.push(summary_100);
                context_recent.push(summary_10);
            }

            Ok(context_recent)
        } else {
            self.sqlite_store.load_recent(session_id, recent_count).await
        }
    }

    /// ── Phase 5: Check if entry is a rolling summary ──
    fn is_rolling_summary_entry(&self, entry: &MemoryEntry) -> bool {
        entry.tags
            .as_ref()
            .map(|tags| tags.iter().any(|tag| tag.starts_with("summary:rolling:")))
            .unwrap_or(false)
    }

    /// ── Phase 5: Select best rolling summaries ──
    fn select_best_rolling_summaries(&self, summaries: Vec<MemoryEntry>) 
        -> (Option<MemoryEntry>, Option<MemoryEntry>) {
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

        (latest_10, latest_100)
    }

    /// ── Phase 5: Merge and deduplicate multi-head search results ──
    fn merge_and_deduplicate_results(
        &self,
        multi_search_result: Vec<(EmbeddingHead, Vec<MemoryEntry>)>,
    ) -> Result<Vec<(EmbeddingHead, MemoryEntry)>> {
        let mut all_candidates = Vec::new();
        let mut content_dedup = HashMap::new();

        for (head, entries) in multi_search_result {
            for entry in entries {
                // Simple deduplication by content hash to avoid identical chunks
                let content_key = format!("{}:{}", 
                    entry.content.len(), 
                    entry.content.chars().take(50).collect::<String>()
                );
                
                if !content_dedup.contains_key(&content_key) {
                    content_dedup.insert(content_key, true);
                    all_candidates.push((head, entry));
                } else {
                    debug!("Deduplicated similar content from {} head", head.as_str());
                }
            }
        }

        debug!("After deduplication: {} candidates", all_candidates.len());
        Ok(all_candidates)
    }

    /// ── Phase 5: Compute composite re-ranking scores ──
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
                0.0
            };

            // Calculate salience score (normalize to 0-1 range)
            let salience_score = entry.salience.unwrap_or(0.0).min(1.0).max(0.0);

            // Calculate recency score (exponential decay from timestamp)
            let hours_ago = (now - entry.timestamp).num_hours().max(0) as f32;
            let recency_score = (-hours_ago / 168.0).exp(); // 168 hours = 1 week half-life

            // Phase 5: Composite score with head-specific weights
            let (sim_weight, sal_weight, rec_weight) = match head {
                EmbeddingHead::Code => (0.70, 0.25, 0.05),    // Favor salience for code
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

    /// ── Phase 5: Cosine similarity calculation for embeddings ──
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

    /// ── Phase 4: Session counters for rolling summaries ──
    pub async fn increment_session_counter(&self, session_id: &str) -> usize {
        let mut counters = self.session_counters.write().await;
        let count = counters.entry(session_id.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    pub async fn get_session_message_count(&self, session_id: &str) -> usize {
        let counters = self.session_counters.read().await;
        counters.get(session_id).cloned().unwrap_or(0)
    }

    /// ── Phase 4: Check and trigger rolling summaries ──
    pub async fn check_and_trigger_rolling_summaries(&self, session_id: &str) -> Result<()> {
        if !CONFIG.is_robust_memory_enabled() {
            return Ok(());
        }

        let message_count = self.get_session_message_count(session_id).await;
        let mut triggered = false;

        // Check for 10-message rolling summary
        if CONFIG.summary_rolling_10 && message_count % 10 == 0 && message_count >= 10 {
            self.create_rolling_summary(session_id, 10).await?;
            triggered = true;
        }

        // Check for 100-message rolling summary
        if CONFIG.summary_rolling_100 && message_count % 100 == 0 && message_count >= 100 {
            self.create_rolling_summary(session_id, 100).await?;
            triggered = true;
        }

        if triggered {
            info!("✅ Triggered rolling summaries at message count {}", message_count);
        }

        Ok(())
    }

    /// ── Phase 4: Create a rolling summary ──
    async fn create_rolling_summary(&self, session_id: &str, n: usize) -> Result<()> {
        info!("📋 Creating {}-message rolling summary for session {}", n, session_id);

        // Load recent messages (excluding existing summaries)
        let all_recent = self.sqlite_store.load_recent(session_id, n * 2).await?;
        let recent_messages: Vec<_> = all_recent
            .into_iter()
            .filter(|msg| !self.is_rolling_summary_entry(msg))
            .take(n)
            .collect();

        if recent_messages.len() < n / 2 {
            debug!("Not enough messages ({}) to create {}-message summary", recent_messages.len(), n);
            return Ok(());
        }

        // Create conversation prompt
        let mut prompt = format!("Please create a concise summary of this conversation (last {} messages):\n\n", n);
        for msg in recent_messages.iter().rev() {
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        // Generate summary
        let token_limit = if n <= 10 { 256 } else { 512 };
        let summary_content = self
            .llm_client
            .summarize_conversation(&prompt, token_limit)
            .await?
            .trim()
            .to_string();

        if summary_content.is_empty() {
            return Ok(());
        }

        // Save the rolling summary
        self.save_rolling_summary(session_id, &summary_content, n)
            .await?;

        info!(
            "✅ Created {}-message rolling summary for session {}",
            n, session_id
        );
        Ok(())
    }

    /// Save a rolling summary with appropriate tags
    async fn save_rolling_summary(
        &self,
        session_id: &str,
        summary_content: &str,
        window_size: usize,
    ) -> Result<()> {
        let rolling_tag = format!("summary:rolling:{}", window_size);

        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "assistant".to_string(),
            content: summary_content.to_string(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(1.5), // Low salience to not dominate retrieval
            tags: Some(vec![
                "summary".to_string(),
                rolling_tag,
                "compressed".to_string(),
            ]),
            summary: Some(format!("Rolling summary of last {} messages", window_size)),
            memory_type: Some(MemoryType::Summary),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: Some(false),
            lang: Some("natural".to_string()),
            topics: Some(vec!["summary".to_string()]),
            pinned: Some(false),
            subject_tag: None,
            last_accessed: Some(Utc::now()),
        };

        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("💾 Saved rolling summary to SQLite");

        // Always embed rolling summaries regardless of salience
        if CONFIG.is_robust_memory_enabled() {
            // Use both Summary and Semantic heads for rolling summaries
            let summary_heads = vec![EmbeddingHead::Summary, EmbeddingHead::Semantic];
            self.generate_and_save_embeddings(&saved_entry, &summary_heads)
                .await?;
        } else {
            // Legacy single-embedding path
            if let Ok(embedding) = self.llm_client.get_embedding(summary_content).await {
                let mut entry_with_embedding = saved_entry;
                entry_with_embedding.embedding = Some(embedding);
                self.qdrant_store.save(&entry_with_embedding).await?;
                info!("💾 Saved rolling summary to single Qdrant collection");
            }
        }

        Ok(())
    }

    /// Save a user message to memory stores.
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        // Increment session counter first
        let message_count = self.increment_session_counter(session_id).await;
        debug!(
            "📈 Session {} message count now: {}",
            session_id, message_count
        );

        let mut entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(self.calculate_user_message_salience(content) as f32),
            tags: Some(vec!["conversational".to_string()]),
            summary: Some(format!(
                "User query: {}",
                content.chars().take(50).collect::<String>()
            )),
            memory_type: Some(MemoryType::Other),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: project_id.map(String::from),
            head: None,
            is_code: None,
            lang: None,
            topics: None,
            pinned: Some(false),
            subject_tag: None,
            last_accessed: Some(Utc::now()),
        };

        // Phase 3: Rich metadata classification (if enabled)
        if CONFIG.is_robust_memory_enabled() {
            info!("🧠 Classifying user message for rich metadata...");
            match self.llm_client.classify_text(content).await {
                Ok(classification) => {
                    entry = entry.with_classification(classification.clone());

                    let mut new_tags = entry.tags.clone().unwrap_or_default();
                    if classification.is_code {
                        new_tags.push("is_code:true".to_string());
                    }
                    if !classification.lang.is_empty() && classification.lang != "natural" {
                        new_tags.push(format!("lang:{}", classification.lang));
                    }
                    for topic in classification.topics {
                        new_tags.push(format!("topic:{}", topic));
                    }
                    entry.tags = Some(new_tags);
                }
                Err(e) => {
                    error!(
                        "Failed to classify message: {}. Proceeding with default metadata.",
                        e
                    );
                }
            }
        }

        // Always save to SQLite first
        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("💾 Saved user message to SQLite");

        // Conditionally generate and save embeddings to Qdrant
        if let Some(salience) = saved_entry.salience {
            if salience >= CONFIG.min_salience_for_qdrant {
                if CONFIG.is_robust_memory_enabled() {
                    let heads_to_use = CONFIG
                        .get_embedding_heads()
                        .into_iter()
                        .filter(|h| h != "summary") // Exclude summary head for user messages
                        .map(|s| EmbeddingHead::from_str(&s))
                        .filter_map(Result::ok)
                        .collect::<Vec<_>>();

                    self.generate_and_save_embeddings(&saved_entry, &heads_to_use)
                        .await?;
                } else {
                    // Legacy single-embedding path
                    if let Ok(embedding) = self.llm_client.get_embedding(content).await {
                        let mut entry_with_embedding = saved_entry;
                        entry_with_embedding.embedding = Some(embedding);
                        self.qdrant_store.save(&entry_with_embedding).await?;
                        info!("💾 Saved user message to single Qdrant collection");
                    }
                }
            }
        }

        Ok(())
    }

    /// Save an assistant response to memory stores.
    pub async fn save_assistant_response(
        &self,
        session_id: &str,
        response: &ChatResponse,
    ) -> Result<()> {
        let message_count = self.increment_session_counter(session_id).await;
        debug!("📈 Session {} message count now: {}", session_id, message_count);

        let mut entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "assistant".to_string(),
            content: response.output.clone(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(response.salience as f32),
            tags: Some(response.tags.clone()),
            summary: Some(response.summary.clone()),
            memory_type: Some(self.parse_memory_type(
                &response.memory_type.as_ref().unwrap_or(&"other".to_string())
            )),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: None,
            lang: None,
            topics: None,
            pinned: Some(false),
            subject_tag: None,
            last_accessed: Some(Utc::now()),
        };

        // Phase 3: Rich metadata classification
        if CONFIG.is_robust_memory_enabled() {
            match self.llm_client.classify_text(&response.output).await {
                Ok(classification) => {
                    entry = entry.with_classification(classification);
                }
                Err(e) => {
                    error!("Failed to classify assistant response: {}", e);
                }
            }
        }

        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("💾 Saved assistant response to SQLite");

        // Generate embeddings if salience threshold is met
        if let Some(salience) = saved_entry.salience {
            if salience >= CONFIG.min_salience_for_qdrant {
                if CONFIG.is_robust_memory_enabled() {
                    let heads_to_use = CONFIG
                        .get_embedding_heads()
                        .into_iter()
                        .filter(|h| h != "summary")
                        .map(|s| EmbeddingHead::from_str(&s))
                        .filter_map(Result::ok)
                        .collect::<Vec<_>>();

                    self.generate_and_save_embeddings(&saved_entry, &heads_to_use)
                        .await?;
                } else {
                    if let Ok(embedding) = self.llm_client.get_embedding(&response.output).await {
                        let mut entry_with_embedding = saved_entry;
                        entry_with_embedding.embedding = Some(embedding);
                        self.qdrant_store.save(&entry_with_embedding).await?;
                        info!("💾 Saved assistant response to single Qdrant collection");
                    }
                }
            }
        }

        // ── Phase 4: Check and trigger rolling summaries ──
        self.check_and_trigger_rolling_summaries(session_id).await?;

        Ok(())
    }

    /// ── Phase 2: Generate and save embeddings for multiple heads ──
    pub async fn generate_and_save_embeddings(
        &self,
        entry: &MemoryEntry,
        heads: &[EmbeddingHead],
    ) -> Result<()> {
        let multi_store = self.multi_store.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Multi-store not available for multi-head embeddings"))?;

        info!("🧠 Generating embeddings for {} heads: {:?}", heads.len(), heads);

        for &head in heads {
            // 1. Chunk the content based on the head type
            let chunker = TextChunker::new(head);
            let chunks = chunker.chunk(&entry.content);
            debug!("Generated {} chunks for {} head", chunks.len(), head.as_str());

            // 2. Generate embeddings for each chunk
            let mut embeddings = Vec::new();
            for chunk_text in &chunks {
                let embedding = self.llm_client.get_embedding(chunk_text).await?;
                embeddings.push(embedding);
            }

            // 3. Save each chunk with its embedding to the appropriate collection
            for (chunk_text, embedding) in chunks.iter().zip(embeddings.iter()) {
                let mut chunk_entry = entry.clone();
                chunk_entry.content = chunk_text.clone();
                chunk_entry.embedding = Some(embedding.clone());
                chunk_entry.head = Some(head.to_string());

                multi_store.save(head, &chunk_entry).await?;
            }

            info!(
                "💾 Saved {} chunks to {} collection for entry {}",
                chunks.len(),
                head,
                entry.id.unwrap_or(-1)
            );
        }

        Ok(())
    }

    /// Calculate salience score for user messages
    fn calculate_user_message_salience(&self, content: &str) -> usize {
        let base_salience = 5;
        let length_bonus = std::cmp::min(content.len() / 100, 3);
        let question_bonus = if content.contains('?') { 2 } else { 0 };
        let code_bonus = if content.contains("```") || content.contains("fn ") || content.contains("def ") {
            3
        } else {
            0
        };

        base_salience + length_bonus + question_bonus + code_bonus
    }

    /// Parse memory type from string
    fn parse_memory_type(&self, memory_type: &str) -> MemoryType {
        match memory_type.to_lowercase().as_str() {
            "feeling" => MemoryType::Feeling,
            "fact" => MemoryType::Fact,
            "joke" => MemoryType::Joke,
            "promise" => MemoryType::Promise,
            "event" => MemoryType::Event,
            "summary" => MemoryType::Summary,
            "context" => MemoryType::Summary,
            _ => MemoryType::Other,
        }
    }

    /// Get recent context from memory (for API compatibility)
    pub async fn get_recent_context(
        &self,
        session_id: &str,
        n: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.sqlite_store.load_recent(session_id, n).await
    }

    /// Search for similar memories using semantic search
    pub async fn search_similar(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        if CONFIG.is_robust_memory_enabled() {
            if let Some(multi_store) = &self.multi_store {
                // Use multi-head search - for now just use semantic head
                if let Ok(query_embedding) = self.llm_client.get_embedding(query).await {
                    multi_store
                        .search(EmbeddingHead::Semantic, "", &query_embedding, limit)
                        .await
                } else {
                    Err(anyhow::anyhow!("Failed to generate embedding for search query"))
                }
            } else {
                // Fallback to single store
                if let Ok(query_embedding) = self.llm_client.get_embedding(query).await {
                    self.qdrant_store.semantic_search("", &query_embedding, limit).await
                } else {
                    Ok(Vec::new())
                }
            }
        } else {
            // Legacy single-head search
            if let Ok(query_embedding) = self.llm_client.get_embedding(query).await {
                self.qdrant_store.semantic_search("", &query_embedding, limit).await
            } else {
                Ok(Vec::new())
            }
        }
    }

    /// Get reference to SQLite store for direct access
    pub fn sqlite_store(&self) -> &Arc<SqliteMemoryStore> {
        &self.sqlite_store
    }

    /// Get reference to Qdrant store for direct access
    pub fn qdrant_store(&self) -> &Arc<QdrantMemoryStore> {
        &self.qdrant_store
    }

    /// Get reference to multi-store if available
    pub fn multi_store(&self) -> Option<&Arc<QdrantMultiStore>> {
        self.multi_store.as_ref()
    }

    /// Check if multi-head mode is enabled and available
    pub fn is_multi_head_enabled(&self) -> bool {
        CONFIG.is_robust_memory_enabled() && self.multi_store.is_some()
    }

    /// ── Phase 5: Evaluate and save response (for DocumentService compatibility) ──
    pub async fn evaluate_and_save_response(
        &self,
        session_id: &str,
        response: &ChatResponse,
        project_id: Option<&str>,
    ) -> Result<()> {
        // Use the existing save_assistant_response method
        self.save_assistant_response(session_id, response).await
    }

    /// ── Phase 5: Get memory service statistics for monitoring ──
    pub async fn get_service_stats(&self, session_id: &str) -> Result<MemoryServiceStats> {
        let recent_count = self.sqlite_store.load_recent(session_id, 1000).await?.len();
        let session_count = self.get_session_message_count(session_id).await;
        
        let semantic_count = if let Some(multi_store) = &self.multi_store {
            // Count entries across all heads
            let mut total = 0;
            for head in multi_store.get_enabled_heads() {
                if let Ok(results) = multi_store.search(head, session_id, &vec![0.0; 1536], 1000).await {
                    total += results.len();
                }
            }
            total
        } else {
            // Single head count (approximate via search)
            if let Ok(embedding) = self.llm_client.get_embedding("test").await {
                self.qdrant_store.semantic_search(session_id, &embedding, 1000).await?.len()
            } else {
                0
            }
        };

        Ok(MemoryServiceStats {
            session_id: session_id.to_string(),
            total_messages: session_count,
            recent_messages: recent_count,
            semantic_entries: semantic_count,
            multi_head_enabled: self.is_multi_head_enabled(),
            heads_available: if let Some(multi_store) = &self.multi_store {
                multi_store.get_enabled_heads().len()
            } else {
                1
            },
        })
    }
}

/// ── Phase 5: Memory service statistics for monitoring ──
#[derive(Debug, Clone)]
pub struct MemoryServiceStats {
    pub session_id: String,
    pub total_messages: usize,
    pub recent_messages: usize,
    pub semantic_entries: usize,
    pub multi_head_enabled: bool,
    pub heads_available: usize,
}
