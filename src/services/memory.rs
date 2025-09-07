// src/services/memory.rs
// Memory service with GPT-5 robust memory system
// Handles memory storage, retrieval, and embeddings

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use futures::future;

use crate::config::CONFIG;
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::{EmbeddingHead, TextChunker};
use crate::memory::{
    qdrant::multi_store::QdrantMultiStore,
    sqlite::store::SqliteMemoryStore,
    traits::MemoryStore,
    types::{MemoryEntry, MemoryType},
    recall::RecallContext,
};
use crate::services::chat::ChatResponse;

#[derive(Debug, Clone)]
pub struct ScoredMemoryEntry {
    pub entry: MemoryEntry,
    pub similarity_score: f32,
    pub salience_score: f32,
    pub recency_score: f32,
    pub composite_score: f32,
    pub source_head: EmbeddingHead,
}

#[derive(Debug, Clone)]
pub struct MemoryServiceStats {
    pub total_messages: usize,
    pub recent_messages: usize,
    pub semantic_entries: usize,
    pub code_entries: usize,
    pub summary_entries: usize,
}

pub struct MemoryService {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<SqliteMemoryStore>,
    multi_store: Arc<QdrantMultiStore>,
    session_counters: Arc<RwLock<HashMap<String, usize>>>,
}

impl MemoryService {
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        info!("MemoryService initialized with multi-collection support");
        Self {
            llm_client,
            sqlite_store,
            multi_store,
            session_counters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn parallel_recall_context(
        &self,
        session_id: &str,
        query_text: &str,
        recent_count: usize,
        semantic_count: usize,
    ) -> Result<RecallContext> {
        info!("Starting parallel recall context for session: {}", session_id);
        self.build_context_with_multihead_parallel(session_id, query_text, recent_count, semantic_count).await
    }

    async fn build_context_with_multihead_parallel(
        &self,
        session_id: &str,
        query_text: &str,
        recent_count: usize,
        semantic_count: usize,
    ) -> Result<RecallContext> {
        debug!("Building context with multi-head parallel retrieval");

        // Parallel execution - embedding + recent messages
        let (embedding_result, recent_result) = tokio::join!(
            self.llm_client.get_embedding(query_text),
            self.load_recent_with_rolling_summaries(session_id, recent_count)
        );

        let embedding = embedding_result?;
        let context_recent = recent_result?;
        debug!("Loaded {} recent messages (including summaries)", context_recent.len());

        // Determine which heads to search based on query classification
        let heads_to_search = self.determine_search_heads(query_text).await;
        
        // Parallel multi-head search using futures
        let search_futures: Vec<_> = heads_to_search
            .iter()
            .map(|&head| {
                let multi_store = self.multi_store.clone();
                let embedding = embedding.clone();
                let session_id = session_id.to_string();
                
                async move {
                    let results = multi_store
                        .search(head, &session_id, &embedding, semantic_count)
                        .await
                        .unwrap_or_else(|e| {
                            warn!("Search failed for {} head: {}", head, e);
                            Vec::new()
                        });
                    (head, results)
                }
            })
            .collect();

        // Execute all searches in parallel
        let multi_search_results = future::join_all(search_futures).await;
        
        info!("Parallel search completed for {} heads", heads_to_search.len());

        // Merge and deduplicate results
        let context_semantic = self.merge_and_deduplicate_results(multi_search_results)?;

        let context = RecallContext {
            recent: context_recent,
            semantic: context_semantic,
        };

        info!(
            "Multi-head parallel context built: {} recent, {} semantic matches",
            context.recent.len(),
            context.semantic.len()
        );

        Ok(context)
    }

    async fn determine_search_heads(&self, query_text: &str) -> Vec<EmbeddingHead> {
        let mut heads = vec![EmbeddingHead::Semantic];
        
        // Classify the query to determine if we should search code head
        match self.llm_client.classify_text(query_text).await {
            Ok(classification) => {
                if classification.is_code {
                    heads.push(EmbeddingHead::Code);
                    debug!("Query classified as code, adding Code head to search");
                }
            }
            Err(e) => {
                debug!("Failed to classify query: {}, using semantic only", e);
            }
        }
        
        // Only search summary head if explicitly configured
        if CONFIG.should_use_rolling_summaries_in_context() {
            heads.push(EmbeddingHead::Summary);
            debug!("Rolling summaries enabled, adding Summary head to search");
        }
        
        heads
    }

    async fn load_recent_with_rolling_summaries(
        &self,
        session_id: &str,
        recent_count: usize,
    ) -> Result<Vec<MemoryEntry>> {
        if CONFIG.should_use_rolling_summaries_in_context() {
            let all_recent = self.sqlite_store.load_recent(session_id, recent_count * 2).await?;
            
            let (summaries, regular): (Vec<_>, Vec<_>) = all_recent.into_iter()
                .partition(|entry| {
                    entry.tags.as_ref()
                        .map(|tags| tags.iter().any(|tag| tag.starts_with("summary:rolling:")))
                        .unwrap_or(false)
                });

            let mut context_recent = Vec::new();
            let immediate_count = std::cmp::min(8, recent_count / 3);
            context_recent.extend(regular.into_iter().take(immediate_count));

            if !summaries.is_empty() {
                context_recent.extend(summaries.into_iter().take(2));
            }

            Ok(context_recent)
        } else {
            self.sqlite_store.load_recent(session_id, recent_count).await
        }
    }

    fn merge_and_deduplicate_results(
        &self,
        multi_search_results: Vec<(EmbeddingHead, Vec<MemoryEntry>)>,
    ) -> Result<Vec<MemoryEntry>> {
        let mut all_candidates = Vec::new();
        let mut content_dedup = HashMap::new();

        for (head, entries) in multi_search_results {
            for entry in entries {
                let content_key = format!("{}:{}", head, entry.content.chars().take(100).collect::<String>());
                
                if !content_dedup.contains_key(&content_key) {
                    content_dedup.insert(content_key.clone(), true);
                    all_candidates.push(entry);
                }
            }
        }

        // Sort by salience and take top results
        all_candidates.sort_by(|a, b| {
            b.salience.unwrap_or(0.0)
                .partial_cmp(&a.salience.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(all_candidates)
    }

    // Batch embedding implementation for performance optimization
    pub async fn generate_and_save_embeddings(
        &self,
        entry: &MemoryEntry,
        heads: &[EmbeddingHead],
    ) -> Result<()> {
        info!("Generating embeddings for {} heads: {:?}", heads.len(), heads);
        let chunker = TextChunker::new()?;

        // Collect all chunks from all heads
        let mut all_chunks: Vec<(EmbeddingHead, String)> = Vec::new();
        
        for &head in heads {
            let chunks = chunker.chunk_text(&entry.content, &head)?;
            debug!("Generated {} chunks for {} head", chunks.len(), head.as_str());
            
            for chunk_text in chunks {
                all_chunks.push((head, chunk_text));
            }
        }

        info!("Total chunks to embed: {}", all_chunks.len());

        // Batch embed all chunks (up to 100 at a time)
        const MAX_BATCH_SIZE: usize = 100;
        let mut all_embeddings: Vec<Vec<f32>> = Vec::new();
        
        for batch_start in (0..all_chunks.len()).step_by(MAX_BATCH_SIZE) {
            let batch_end = std::cmp::min(batch_start + MAX_BATCH_SIZE, all_chunks.len());
            let batch_texts: Vec<String> = all_chunks[batch_start..batch_end]
                .iter()
                .map(|(_, text)| text.clone())
                .collect();
            
            debug!("Processing batch {}-{} of {}", 
                batch_start + 1, batch_end, all_chunks.len());
            
            // Use batch embedding instead of sequential
            let batch_embeddings = self.llm_client
                .embedding_client()
                .get_embeddings_batch(&batch_texts)
                .await?;
            
            all_embeddings.extend(batch_embeddings);
            
            info!("Embedded batch of {} chunks", batch_texts.len());
        }

        // Save all chunks with their embeddings to appropriate collections
        for ((head, chunk_text), embedding) in all_chunks.iter().zip(all_embeddings.iter()) {
            let mut chunk_entry = entry.clone();
            chunk_entry.content = chunk_text.clone();
            chunk_entry.embedding = Some(embedding.clone());
            chunk_entry.head = Some(head.to_string());

            self.multi_store.save(*head, &chunk_entry).await?;
        }

        info!(
            "Saved {} total chunks across {} heads using batch embedding",
            all_chunks.len(),
            heads.len()
        );

        Ok(())
    }

    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        // Increment session counter first
        let message_count = self.increment_session_counter(session_id).await;
        debug!("Session {} message count now: {}", session_id, message_count);

        // Rich metadata classification with GPT-5
        info!("Classifying user message for rich metadata");
        let classification_result = self.llm_client.classify_text(content).await;
        
        let (salience, is_code, classification) = match classification_result {
            Ok(c) => (c.salience, c.is_code, Some(c)),
            Err(e) => {
                error!("Failed to classify message: {}. Using defaults.", e);
                (0.5, false, None)
            }
        };

        let mut entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(salience), // Use GPT-5 provided salience
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

        if let Some(classification) = classification {
            entry = entry.with_classification(classification.clone());

            let mut new_tags = entry.tags.clone().unwrap_or_default();
            if classification.is_code {
                new_tags.push("is_code:true".to_string());
            }
            if !classification.lang.is_empty() && classification.lang != "natural" {
                new_tags.push(format!("lang:{}", classification.lang));
            }
            for topic in classification.topics {
                new_tags.push(format!("topic:{topic}"));
            }
            entry.tags = Some(new_tags);
        }

        // Always save to SQLite first
        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("Saved user message to SQLite");

        // Dynamic head selection based on classification
        if let Some(salience) = saved_entry.salience {
            if salience >= CONFIG.min_salience_for_qdrant {
                let mut heads_to_use = vec![EmbeddingHead::Semantic];
                
                // Only add code head if content is actually code
                if is_code {
                    heads_to_use.push(EmbeddingHead::Code);
                    info!("Content classified as code, adding Code head");
                }
                
                // Never use summary head for user messages
                self.generate_and_save_embeddings(&saved_entry, &heads_to_use)
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn save_assistant_response(
        &self,
        session_id: &str,
        response: &ChatResponse,
    ) -> Result<()> {
        let message_count = self.increment_session_counter(session_id).await;
        debug!("Session {} message count now: {}", session_id, message_count);

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
                if response.memory_type.trim().is_empty() {
                    "other"
                } else {
                    response.memory_type.as_str()
                },
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

        // Rich metadata classification
        let classification_result = self.llm_client.classify_text(&response.output).await;
        let is_code = match &classification_result {
            Ok(c) => c.is_code,
            Err(e) => {
                error!("Failed to classify assistant response: {}", e);
                false
            }
        };

        if let Ok(classification) = classification_result {
            entry = entry.with_classification(classification);
        }

        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("Saved assistant response to SQLite");

        // Dynamic head selection for assistant responses
        if let Some(salience) = saved_entry.salience {
            if salience >= CONFIG.min_salience_for_qdrant {
                let mut heads_to_use = vec![EmbeddingHead::Semantic];
                
                if is_code {
                    heads_to_use.push(EmbeddingHead::Code);
                }

                self.generate_and_save_embeddings(&saved_entry, &heads_to_use)
                    .await?;
            }
        }

        // Check and trigger rolling summaries
        self.check_and_trigger_rolling_summaries(session_id).await?;

        Ok(())
    }

    async fn check_and_trigger_rolling_summaries(&self, session_id: &str) -> Result<()> {
        let count = self.get_session_message_count(session_id).await;
        
        if CONFIG.rolling_10_enabled() && count % 10 == 0 {
            info!("Triggering 10-message rolling summary for session {}", session_id);
            self.create_rolling_summary(session_id, 10).await?;
        }
        
        if CONFIG.rolling_100_enabled() && count % 100 == 0 {
            info!("Triggering 100-message rolling summary for session {}", session_id);
            self.create_rolling_summary(session_id, 100).await?;
        }
        
        Ok(())
    }

    pub async fn create_rolling_summary(&self, session_id: &str, window_size: usize) -> Result<()> {
        let messages = self.sqlite_store.load_recent(session_id, window_size).await?;
        
        if messages.len() < window_size {
            debug!("Not enough messages for {}-message rolling summary", window_size);
            return Ok(());
        }
        
        // Prepare content for summarization
        let content = messages.iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        
        // Generate summary
        let summary_prompt = format!(
            "Create a concise rolling summary of the last {window_size} messages:\n\n{content}"
        );
        
        let summary = self.llm_client.summarize_conversation(&summary_prompt, 500).await?;
        
        // Save summary as a special memory entry
        let summary_entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "system".to_string(),
            content: summary.clone(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(1.0),
            tags: Some(vec![
                "summary".to_string(),
                format!("summary:rolling:{}", window_size),
                "system".to_string(),
            ]),
            summary: Some(summary),
            memory_type: Some(MemoryType::Summary),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: Some(false),
            lang: None,
            topics: None,
            pinned: Some(false),
            subject_tag: None,
            last_accessed: Some(Utc::now()),
        };
        
        self.sqlite_store.save(&summary_entry).await?;
        info!("Created {}-message rolling summary for session {}", window_size, session_id);
        
        Ok(())
    }

    pub async fn create_snapshot_summary(&self, session_id: &str) -> Result<()> {
        let message_count = self.get_session_message_count(session_id).await;
        info!("Creating snapshot summary at message count {}", message_count);
        
        // Get recent messages for summarization
        let recent_messages = self.get_recent_context(session_id, 20).await?;
        
        if recent_messages.len() < 10 {
            info!("Not enough messages for summary ({}), skipping", recent_messages.len());
            return Ok(());
        }
        
        // Build summary prompt
        let mut context_text = String::new();
        for msg in &recent_messages {
            context_text.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }
        
        let summary_prompt = format!(
            "Create a concise summary of the following conversation:\n\n{context_text}"
        );
        
        // Generate summary
        let summary = self.llm_client
            .simple_chat(&summary_prompt, "gpt-4o", "You are a summarization assistant.")
            .await?;
        
        // Save as snapshot summary
        let snapshot_entry = ChatResponse {
            output: String::new(),
            persona: "system".to_string(),
            mood: "analytical".to_string(),
            salience: 2,
            summary,
            memory_type: "summary".to_string(),
            tags: vec![
                "summary".to_string(),
                format!("summary:snapshot:{}", message_count),
                "manual".to_string(),
            ],
            intent: Some("snapshot_summary".to_string()),
            monologue: None,
            reasoning_summary: None,
        };
        
        self.save_assistant_response(session_id, &snapshot_entry).await?;
        
        info!("Created snapshot summary for session {}", session_id);
        Ok(())
    }

    fn parse_memory_type(&self, memory_type: &str) -> MemoryType {
        match memory_type.to_lowercase().as_str() {
            "feeling" => MemoryType::Feeling,
            "fact" => MemoryType::Fact,
            "joke" => MemoryType::Joke,
            "promise" => MemoryType::Promise,
            "event" => MemoryType::Event,
            "summary" => MemoryType::Summary,
            _ => MemoryType::Other,
        }
    }

    async fn increment_session_counter(&self, session_id: &str) -> usize {
        let mut counters = self.session_counters.write().await;
        let count = counters.entry(session_id.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    pub async fn get_session_message_count(&self, session_id: &str) -> usize {
        let counters = self.session_counters.read().await;
        counters.get(session_id).copied().unwrap_or(0)
    }

    pub async fn get_recent_context(
        &self,
        session_id: &str,
        n: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.sqlite_store.load_recent(session_id, n).await
    }

    // FIXED: Now accepts session_id as a parameter instead of using CONFIG.session_id!
    pub async fn search_similar(
        &self,
        session_id: &str,  // <-- Added parameter
        query: &str,
        limit: usize
    ) -> Result<Vec<MemoryEntry>> {
        let query_embedding = self.llm_client.get_embedding(query).await?;
        self.multi_store.search(
            EmbeddingHead::Semantic,
            session_id,  // <-- Use the passed session_id instead of CONFIG.session_id
            &query_embedding,
            limit
        ).await
    }

    pub async fn get_service_stats(&self, session_id: &str) -> Result<MemoryServiceStats> {
        let total = self.get_session_message_count(session_id).await;
        let recent = self.sqlite_store.load_recent(session_id, 100).await?.len();
        
        Ok(MemoryServiceStats {
            total_messages: total,
            recent_messages: recent,
            semantic_entries: 0,
            code_entries: 0,
            summary_entries: 0,
        })
    }
}
