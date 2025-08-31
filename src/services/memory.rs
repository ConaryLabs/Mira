// src/services/memory.rs
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::CONFIG;
use crate::llm::client::OpenAIClient; // Added this import
use crate::llm::embeddings::{EmbeddingHead, TextChunker};
use crate::memory::{
    qdrant::{multi_store::QdrantMultiStore, store::QdrantMemoryStore},
    sqlite::store::SqliteMemoryStore,
    traits::MemoryStore,
    types::{MemoryEntry, MemoryType},
};
use crate::services::chat::ChatResponse;

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
    pub async fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
    ) -> Result<Self> {
        let multi_store = if CONFIG.is_robust_memory_enabled() {
            // Get the base URL from the qdrant_store config
            let base_url = &CONFIG.qdrant_url;
            let collection_base = "mira-memory";
            Some(Arc::new(
                QdrantMultiStore::new(base_url, collection_base).await?,
            ))
        } else {
            None
        };

        Ok(Self {
            llm_client,
            sqlite_store,
            qdrant_store,
            multi_store,
            session_counters: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Alternative constructor for compatibility with existing state management
    pub fn new_with_multi_store(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            qdrant_store,
            multi_store: Some(multi_store),
            session_counters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the current message count for a session
    pub async fn get_session_message_count(&self, session_id: &str) -> usize {
        let counters = self.session_counters.read().await;
        counters.get(session_id).copied().unwrap_or(0)
    }

    /// Increment message counter for a session and return the new count
    async fn increment_session_counter(&self, session_id: &str) -> usize {
        let mut counters = self.session_counters.write().await;
        let count = counters.entry(session_id.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    /// Check if rolling summarization should trigger at current message count
    fn should_trigger_rolling_summary(&self, message_count: usize) -> (bool, bool) {
        let trigger_10 =
            CONFIG.summary_rolling_10 && message_count % 10 == 0 && message_count >= 10;
        let trigger_100 =
            CONFIG.summary_rolling_100 && message_count % 100 == 0 && message_count >= 100;

        // Avoid double-summarizing at 100 (since it's also a multiple of 10)
        // If both are triggered, prefer the 100-message summary
        let final_trigger_10 = trigger_10 && !trigger_100;

        (final_trigger_10, trigger_100)
    }

    /// Trigger rolling summarization if conditions are met
    pub async fn check_and_trigger_rolling_summaries(&self, session_id: &str) -> Result<()> {
        if !CONFIG.is_robust_memory_enabled() {
            return Ok(());
        }

        let message_count = self.get_session_message_count(session_id).await;
        let (trigger_10, trigger_100) = self.should_trigger_rolling_summary(message_count);

        if trigger_10 {
            info!(
                "🔄 Triggering 10-message rolling summary for session {} at count {}",
                session_id, message_count
            );
            if let Err(e) = self.create_rolling_summary(session_id, 10).await {
                warn!("⚠️ Failed to create 10-message rolling summary: {}", e);
            }
        }

        if trigger_100 {
            info!(
                "🔄 Triggering 100-message rolling summary for session {} at count {}",
                session_id, message_count
            );
            if let Err(e) = self.create_rolling_summary(session_id, 100).await {
                warn!("⚠️ Failed to create 100-message rolling summary: {}", e);
            }
        }

        Ok(())
    }

    /// Create a rolling summary for the last N messages
    async fn create_rolling_summary(&self, session_id: &str, n: usize) -> Result<()> {
        // Fetch the last N non-summary messages
        let recent_messages = self.sqlite_store.load_recent(session_id, n * 2).await?;

        // Filter out existing summary messages to avoid summarizing summaries
        let non_summary_messages: Vec<_> = recent_messages
            .into_iter()
            .filter(|msg| {
                !msg.tags
                    .as_ref()
                    .map(|tags| tags.iter().any(|tag| tag.contains("summary")))
                    .unwrap_or(false)
            })
            .take(n)
            .collect();

        if non_summary_messages.len() < std::cmp::min(n, 3) {
            debug!("Not enough messages for {}-message rolling summary", n);
            return Ok(());
        }

        // Build prompt for summarization
        let mut prompt = if n <= 10 {
            format!(
                "Summarize the following last {} exchanges briefly to maintain context.\n\
                 Keep it faithful, concise, and useful for context stitching.\n\n",
                non_summary_messages.len()
            )
        } else {
            format!(
                "Summarize the following {} messages at a high level, focusing on key topics and themes.\n\
                 Keep it concise and capture the essential context for long-term recall.\n\n",
                non_summary_messages.len()
            )
        };

        // Reverse to get chronological order (oldest first)
        for msg in non_summary_messages.iter().rev() {
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
            role: "assistant".to_string(), // Use assistant role for rolling summaries
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
            memory_type: Some(MemoryType::Other), // Use Other instead of Episodic
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
                    // Update the entry with the classification results.
                    entry = entry.with_classification(classification.clone());

                    // Overwrite default tags with new, richer tags from the classification.
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

        // Always save the (potentially enriched) message to SQLite first.
        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("💾 Saved user message to SQLite");

        // Conditionally generate and save embeddings to Qdrant.
        if let Some(salience) = saved_entry.salience {
            if salience >= CONFIG.min_salience_for_qdrant {
                if CONFIG.is_robust_memory_enabled() {
                    let heads_to_use = CONFIG
                        .get_embedding_heads()
                        .into_iter()
                        .filter(|h| h != "summary") // Exclude summary head for user messages.
                        .map(|s| EmbeddingHead::from_str(&s)) // Fixed: borrow s
                        .filter_map(Result::ok)
                        .collect::<Vec<_>>();

                    self.generate_and_save_embeddings(&saved_entry, &heads_to_use)
                        .await?;
                } else {
                    // Legacy single-embedding path.
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
        // Increment session counter first
        let message_count = self.increment_session_counter(session_id).await;
        debug!(
            "📈 Session {} message count now: {}",
            session_id, message_count
        );

        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "assistant".to_string(),
            content: response.output.clone(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(response.salience as f32),
            tags: Some(response.tags.clone()),
            summary: Some(response.summary.clone()),
            memory_type: Some(self.parse_memory_type(&response.memory_type)),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: None,
            lang: None,
            topics: None,

            // Phase 4 additions
            pinned: Some(false),
            subject_tag: None,
            last_accessed: Some(Utc::now()),
        };

        // Save to SQLite first to get an ID and the final state of the entry
        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("💾 Saved assistant response to SQLite");

        if let Some(salience) = saved_entry.salience {
            if salience >= CONFIG.min_salience_for_qdrant {
                if CONFIG.is_robust_memory_enabled() {
                    let is_summary = response.tags.contains(&"summary".to_string())
                        || response.memory_type.to_lowercase().contains("summary");

                    let heads_to_use = CONFIG
                        .get_embedding_heads()
                        .into_iter()
                        .filter(|h| if h == "summary" { is_summary } else { true })
                        .map(|s| EmbeddingHead::from_str(&s)) // Fixed: borrow s
                        .filter_map(Result::ok)
                        .collect::<Vec<_>>();

                    self.generate_and_save_embeddings(&saved_entry, &heads_to_use)
                        .await?;
                } else {
                    // Legacy single-embedding path
                    if let Ok(embedding) = self.llm_client.get_embedding(&response.output).await {
                        let mut entry_with_embedding = saved_entry;
                        entry_with_embedding.embedding = Some(embedding);
                        self.qdrant_store.save(&entry_with_embedding).await?;
                        info!("💾 Saved assistant response to single Qdrant collection");
                    }
                }
            }
        }

        // ── Phase 4: Check for rolling summarization after saving ──
        if CONFIG.is_robust_memory_enabled() {
            // Don't trigger rolling summaries for rolling summary messages themselves
            let is_rolling_summary = response
                .tags
                .iter()
                .any(|tag| tag.starts_with("summary:rolling:"));

            if !is_rolling_summary {
                self.check_and_trigger_rolling_summaries(session_id).await?;
            }
        }

        Ok(())
    }

    /// Save a summary to memory stores.
    pub async fn save_summary(
        &self,
        session_id: &str,
        summary_content: &str,
        original_message_count: usize,
    ) -> Result<()> {
        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "system".to_string(),
            content: summary_content.to_string(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(2.0),
            tags: Some(vec!["summary".to_string(), "compressed".to_string()]),
            summary: Some(format!(
                "Summary of previous {} messages",
                original_message_count
            )),
            memory_type: Some(MemoryType::Summary),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: Some(false),
            lang: Some("natural".to_string()),
            topics: Some(vec!["summary".to_string()]),

            // Phase 4 additions
            pinned: Some(false),
            subject_tag: None,
            last_accessed: Some(Utc::now()),
        };

        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("💾 Saved summary to SQLite");

        if CONFIG.is_robust_memory_enabled() {
            // Use both Semantic and Summary heads for summaries
            let summary_heads = vec![EmbeddingHead::Summary, EmbeddingHead::Semantic];
            self.generate_and_save_embeddings(&saved_entry, &summary_heads)
                .await?;
        } else {
            // Legacy single-embedding path
            if let Ok(embedding) = self.llm_client.get_embedding(summary_content).await {
                let mut entry_with_embedding = saved_entry;
                entry_with_embedding.embedding = Some(embedding);
                self.qdrant_store.save(&entry_with_embedding).await?;
                info!("💾 Saved summary to single Qdrant collection");
            }
        }

        Ok(())
    }

    /// Chunks text, generates batch embeddings, and saves them to the correct Qdrant collections.
    async fn generate_and_save_embeddings(
        &self,
        entry: &MemoryEntry,
        heads: &[EmbeddingHead],
    ) -> Result<()> {
        let multi_store = self
            .multi_store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Multi-store not initialized"))?;

        for head in heads {
            // 1. Chunk the content according to the head's parameters.
            let chunker = TextChunker::new()?;
            let chunks = chunker.chunk_text(&entry.content, head)?;

            // 2. Generate embeddings for all chunks (batch processing).
            let mut embeddings = Vec::new();
            for chunk in &chunks {
                // Fixed: Add explicit type annotation
                let embedding: Vec<f32> = self.llm_client.get_embedding(chunk).await?;
                embeddings.push(embedding);
            }

            // 3. Save each chunk with its embedding to the appropriate collection.
            for (chunk_text, embedding) in chunks.iter().zip(embeddings.iter()) {
                let mut chunk_entry = entry.clone();
                chunk_entry.content = chunk_text.clone();
                chunk_entry.embedding = Some(embedding.clone());
                chunk_entry.head = Some(head.to_string());

                multi_store.save(*head, &chunk_entry).await?;
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

        // Boost salience for questions
        let question_bonus = if content.contains('?') { 2 } else { 0 };

        // Boost for code-like content
        let code_bonus = if content.contains("```")
            || content.contains("fn ")
            || content.contains("def ")
        {
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
                    // Note: QdrantMultiStore.search method signature is: search(head, session_id, embedding, limit)
                    multi_store
                        .search(EmbeddingHead::Semantic, "", &query_embedding, limit)
                        .await
                } else {
                    Err(anyhow::anyhow!("Failed to generate query embedding"))
                }
            } else {
                Err(anyhow::anyhow!("Multi-store not available"))
            }
        } else {
            // Legacy single-collection search
            if let Ok(query_embedding) = self.llm_client.get_embedding(query).await {
                self.qdrant_store
                    .search_similar_memories("", &query_embedding, limit)
                    .await
            } else {
                Err(anyhow::anyhow!("Failed to generate query embedding"))
            }
        }
    }

    /// Add missing method for compatibility
    pub async fn evaluate_and_save_response(
        &self,
        session_id: &str,
        response: &crate::services::chat::ChatResponse,
        _project_id: Option<&str>,
    ) -> Result<()> {
        self.save_assistant_response(session_id, response).await
    }

    /// Reset session counter (useful for testing or session cleanup)
    pub async fn reset_session_counter(&self, session_id: &str) {
        let mut counters = self.session_counters.write().await;
        counters.remove(session_id);
        debug!("🔄 Reset message counter for session {}", session_id);
    }

    /// Get all session counters (for debugging/monitoring)
    pub async fn get_all_session_counters(&self) -> HashMap<String, usize> {
        self.session_counters.read().await.clone()
    }
}
