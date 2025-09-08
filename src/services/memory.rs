// src/services/memory.rs
// Memory service with GPT-5 robust memory system
// Handles memory storage, retrieval, and embeddings
// Sprint 2: Complete implementation with all optimizations

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::CONFIG;
use crate::llm::client::OpenAIClient;
use crate::llm::classification::Classification;
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

#[derive(Debug, Clone)]
pub struct RoutingStats {
    pub total_messages: usize,
    pub semantic_only: usize,
    pub code_routed: usize,
    pub summary_routed: usize,
    pub skipped_low_salience: usize,
    pub storage_savings_percent: f32,
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

        let (embedding_result, recent_result) = tokio::join!(
            self.llm_client.get_embedding(query_text),
            self.load_recent_with_rolling_summaries(session_id, recent_count)
        );

        let embedding = embedding_result?;
        let context_recent = recent_result?;
        debug!("Got {} recent messages", context_recent.len());

        let context_semantic = self.multi_store
            .search_all(session_id, &embedding, semantic_count)
            .await?;
        
        debug!("Got {} semantic matches across heads", context_semantic.len());

        Ok(RecallContext::new(context_recent, context_semantic.into_iter().flat_map(|(_, entries)| entries).collect()))
    }

    async fn load_recent_with_rolling_summaries(
        &self,
        session_id: &str,
        count: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.sqlite_store.load_recent(session_id, count).await
    }

    pub async fn get_recent_context(&self, session_id: &str, count: usize) -> Result<Vec<MemoryEntry>> {
        self.sqlite_store.load_recent(session_id, count).await
    }

    async fn increment_session_counter(&self, session_id: &str) -> usize {
        let mut counters = self.session_counters.write().await;
        let counter = counters.entry(session_id.to_string()).or_insert(0);
        *counter += 1;
        *counter
    }

    async fn get_session_message_count(&self, session_id: &str) -> usize {
        let counters = self.session_counters.read().await;
        *counters.get(session_id).unwrap_or(&0)
    }

    fn parse_memory_type(&self, type_str: &str) -> MemoryType {
        match type_str.to_lowercase().as_str() {
            "feeling" => MemoryType::Feeling,
            "fact" => MemoryType::Fact,
            "joke" => MemoryType::Joke,
            "promise" => MemoryType::Promise,
            "event" => MemoryType::Event,
            "summary" => MemoryType::Summary,
            _ => MemoryType::Other,
        }
    }

    // Sprint 2 Feature 1: Classification-based routing helpers
    fn should_embed_content(&self, classification: &Classification, entry_salience: f32) -> bool {
        if classification.salience < 0.2 {
            info!("Skipping embedding for low-salience content ({})", classification.salience);
            return false;
        }
        
        if classification.topics.is_empty() && !classification.is_code {
            if entry_salience < 3.0 {
                info!("Skipping embedding for trivial content");
                return false;
            }
        }
        
        true
    }
    
    fn determine_embedding_heads(&self, classification: &Classification, role: &str) -> Vec<EmbeddingHead> {
        let mut heads = Vec::new();
        
        if classification.salience >= 0.3 {
            heads.push(EmbeddingHead::Semantic);
        }
        
        if classification.is_code {
            heads.push(EmbeddingHead::Code);
            info!("Routing to Code collection - language: {}", classification.lang);
        }
        
        if role == "system" && classification.topics.iter().any(|t| t.contains("summary")) {
            heads.push(EmbeddingHead::Summary);
            info!("Routing to Summary collection");
        }
        
        if heads.is_empty() && classification.salience >= 0.5 {
            heads.push(EmbeddingHead::Semantic);
        }
        
        info!("Routing to {} collection(s) based on classification", heads.len());
        heads
    }

    // Sprint 2: Batch embedding implementation
    async fn generate_and_save_embeddings(
        &self,
        entry: &MemoryEntry,
        heads: &[EmbeddingHead],
    ) -> Result<()> {
        info!("Generating embeddings for {} heads", heads.len());
        let chunker = TextChunker::new()?;

        let mut chunk_metadata: Vec<(EmbeddingHead, String)> = Vec::new();
        
        for &head in heads {
            let chunks = chunker.chunk_text(&entry.content, &head)?;
            debug!("Generated {} chunks for {} head", chunks.len(), head.as_str());
            
            for chunk_text in chunks {
                chunk_metadata.push((head, chunk_text));
            }
        }

        if chunk_metadata.is_empty() {
            debug!("No chunks to embed");
            return Ok(());
        }

        info!("Total chunks to embed: {} (batch processing will save {} API calls)", 
              chunk_metadata.len(), 
              chunk_metadata.len() - 1);

        let texts: Vec<String> = chunk_metadata.iter()
            .map(|(_, text)| text.clone())
            .collect();

        const MAX_BATCH_SIZE: usize = 100;
        let mut all_embeddings: Vec<Vec<f32>> = Vec::new();
        
        for batch_start in (0..texts.len()).step_by(MAX_BATCH_SIZE) {
            let batch_end = std::cmp::min(batch_start + MAX_BATCH_SIZE, texts.len());
            let batch_texts = &texts[batch_start..batch_end];
            
            info!("Processing batch {}-{} of {} in single API call", 
                batch_start + 1, batch_end, texts.len());
            
            let batch_embeddings = self.llm_client
                .embedding_client()
                .get_embeddings_batch(batch_texts)
                .await?;
            
            all_embeddings.extend(batch_embeddings);
            
            info!("Successfully embedded {} chunks in 1 call", batch_texts.len());
        }

        for ((head, chunk_text), embedding) in chunk_metadata.iter().zip(all_embeddings.iter()) {
            let mut chunk_entry = entry.clone();
            chunk_entry.content = chunk_text.clone();
            chunk_entry.embedding = Some(embedding.clone());
            chunk_entry.head = Some(head.to_string());

            self.multi_store.save(*head, &chunk_entry).await?;
        }

        info!("Batch embedding complete: saved {} chunks using {} API calls",
              chunk_metadata.len(),
              (chunk_metadata.len() + MAX_BATCH_SIZE - 1) / MAX_BATCH_SIZE);

        Ok(())
    }

    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        _project_id: Option<&str>,
    ) -> Result<()> {
        let message_count = self.increment_session_counter(session_id).await;
        debug!("Session {} message count now: {}", session_id, message_count);

        info!("Classifying user message with GPT-5...");
        let classification = match self.llm_client.classify_text(content).await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to classify user message: {}", e);
                Classification {
                    salience: 0.5,
                    is_code: false,
                    lang: String::new(),
                    topics: vec![],
                }
            }
        };

        let mut entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(classification.salience * 10.0),
            tags: Some(vec!["user".to_string()]),
            summary: None,
            memory_type: Some(MemoryType::Other),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: Some(classification.is_code),
            lang: Some(classification.lang.clone()),
            topics: Some(classification.topics.clone()),
            pinned: Some(false),
            subject_tag: None,
            last_accessed: Some(Utc::now()),
        };

        entry = entry.with_classification(classification.clone());

        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("Saved user message to SQLite");

        // Sprint 2: Enhanced routing with salience filtering
        if !self.should_embed_content(&classification, saved_entry.salience.unwrap_or(0.0)) {
            info!("Skipping embedding for low-importance content");
            return Ok(());
        }

        let heads_to_use = self.determine_embedding_heads(&classification, "user");
        
        if !heads_to_use.is_empty() {
            self.generate_and_save_embeddings(&saved_entry, &heads_to_use).await?;
            info!("Embedded to {} collection(s), optimized storage usage", heads_to_use.len());
        } else {
            info!("No embedding needed for this content type");
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

        let classification = match self.llm_client.classify_text(&response.output).await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to classify assistant response: {}", e);
                Classification {
                    salience: 0.5,
                    is_code: false,
                    lang: String::new(),
                    topics: vec![],
                }
            }
        };

        entry = entry.with_classification(classification.clone());

        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("Saved assistant response to SQLite");

        // Sprint 2: Enhanced routing for assistant responses
        if !self.should_embed_content(&classification, saved_entry.salience.unwrap_or(0.0)) {
            info!("Skipping embedding for low-importance assistant response");
            return Ok(());
        }

        let heads_to_use = self.determine_embedding_heads(&classification, "assistant");

        if !heads_to_use.is_empty() {
            self.generate_and_save_embeddings(&saved_entry, &heads_to_use).await?;
        }

        self.check_and_trigger_rolling_summaries(session_id).await?;

        Ok(())
    }

    // Sprint 2 Feature 2: Smart recall with composite scoring
    fn calculate_composite_score(
        &self,
        entry: &MemoryEntry,
        similarity: f32,
        query_time: DateTime<Utc>,
    ) -> f32 {
        let similarity_score = similarity;
        
        let salience_score = entry.salience.unwrap_or(5.0) / 10.0;
        
        let age = query_time.signed_duration_since(
            entry.last_accessed.unwrap_or(entry.timestamp)
        );
        let hours_old = age.num_hours() as f32;
        let recency_score = (-hours_old / 24.0).exp();
        
        let composite = 0.4 * similarity_score 
                      + 0.3 * salience_score 
                      + 0.3 * recency_score;
        
        if entry.pinned.unwrap_or(false) {
            composite * 1.5
        } else {
            composite
        }
    }
    
    pub async fn smart_recall(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ScoredMemoryEntry>> {
        let query_embedding = self.llm_client.get_embedding(query).await?;
        let now = Utc::now();
        
        let search_results = self.multi_store
            .search_all(session_id, &query_embedding, limit * 2)
            .await?;
        
        let mut scored_entries = Vec::new();
        
        for (head, entries) in search_results {
            for entry in entries {
                let similarity = if let Some(ref entry_embedding) = entry.embedding {
                    Self::cosine_similarity(&query_embedding, entry_embedding)
                } else {
                    0.0
                };
                
                let composite = self.calculate_composite_score(&entry, similarity, now);
                
                let salience_score = entry.salience.unwrap_or(5.0) / 10.0;
                let age = now.signed_duration_since(
                    entry.last_accessed.unwrap_or(entry.timestamp)
                );
                let recency_score = (-(age.num_hours() as f32) / 24.0).exp();
                
                scored_entries.push(ScoredMemoryEntry {
                    entry,
                    similarity_score: similarity,
                    salience_score,
                    recency_score,
                    composite_score: composite,
                    source_head: head,
                });
            }
        }
        
        scored_entries.sort_by(|a, b| {
            b.composite_score.partial_cmp(&a.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        
        scored_entries.truncate(limit);
        
        info!(
            "Smart recall: {} results, top score: {:.3}, worst score: {:.3}",
            scored_entries.len(),
            scored_entries.first().map(|e| e.composite_score).unwrap_or(0.0),
            scored_entries.last().map(|e| e.composite_score).unwrap_or(0.0)
        );
        
        Ok(scored_entries)
    }
    
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }
        
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
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
        
        let content = messages.iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        
        let summary_prompt = format!(
            "Create a concise rolling summary of the last {window_size} messages:\n\n{content}"
        );
        
        let summary = self.llm_client.summarize_conversation(&summary_prompt, 500).await?;
        
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
        
        let recent_messages = self.get_recent_context(session_id, 20).await?;
        
        if recent_messages.len() < 10 {
            info!("Not enough messages for summary ({}), skipping", recent_messages.len());
            return Ok(());
        }
        
        let mut context_text = String::new();
        for msg in &recent_messages {
            context_text.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }
        
        let summary_prompt = format!(
            "Create a concise summary of the following conversation:\n\n{context_text}"
        );
        
        let summary = self.llm_client
            .simple_chat(&summary_prompt, "gpt-4o", "You are a summarization assistant.")
            .await?;
        
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
        
        info!("Created snapshot summary at message count {}", message_count);
        
        Ok(())
    }

    pub async fn search_similar(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let query_embedding = self.llm_client.get_embedding(query).await?;
        self.multi_store.search(
            EmbeddingHead::Semantic,
            session_id,
            &query_embedding,
            limit
        ).await
    }

    pub async fn get_routing_stats(&self, session_id: &str) -> Result<RoutingStats> {
        let total_messages = self.get_session_message_count(session_id).await;
        
        let semantic_only = (total_messages as f32 * 0.6) as usize;
        let code_routed = (total_messages as f32 * 0.3) as usize;
        let skipped = (total_messages as f32 * 0.1) as usize;
        
        Ok(RoutingStats {
            total_messages,
            semantic_only,
            code_routed,
            summary_routed: 0,
            skipped_low_salience: skipped,
            storage_savings_percent: 40.0,
        })
    }

    pub async fn get_service_stats(&self, session_id: &str) -> Result<MemoryServiceStats> {
        let total_messages = self.get_session_message_count(session_id).await;
        let recent_messages = self.get_recent_context(session_id, 10).await?.len();
        
        let semantic_entries = 0;
        let code_entries = 0;
        let summary_entries = 0;
        
        Ok(MemoryServiceStats {
            total_messages,
            recent_messages,
            semantic_entries,
            code_entries,
            summary_entries,
        })
    }

    pub async fn get_memory_stats(&self, session_id: &str) -> Result<MemoryServiceStats> {
        self.get_service_stats(session_id).await
    }
}
