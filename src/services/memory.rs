// src/services/memory.rs

use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tracing::{debug, error, info, warn};

use crate::config::CONFIG;
use crate::llm::client::OpenAIClient;
use crate::llm::classification::Classification; // Added for Phase 3
use crate::llm::embeddings::{EmbeddingHead, TextChunker};
use crate::memory::qdrant::multi_store::QdrantMultiStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};
use crate::services::chat::ChatResponse;

/// Unified memory service that manages both SQLite and Qdrant stores.
/// It now supports multi-head embeddings and write-time classification.
pub struct MemoryService {
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_store: Arc<QdrantMemoryStore>,
    qdrant_multi_store: Arc<QdrantMultiStore>,
    llm_client: Arc<OpenAIClient>,
    text_chunker: TextChunker,
}

impl MemoryService {
    /// Constructor with multi-store support.
    pub fn new_with_multi_store(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        qdrant_multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self {
            sqlite_store,
            qdrant_store,
            qdrant_multi_store,
            llm_client,
            text_chunker: TextChunker::new().expect("Failed to initialize text chunker"),
        }
    }

    /// Save a user message, enriching it with classification and multi-head embeddings.
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        // Create a basic entry from the message content.
        let mut entry = MemoryEntry::from_user_message(session_id.to_string(), content.to_string());

        if let Some(proj_id) = project_id {
            if let Some(ref mut tags) = entry.tags {
                tags.push(format!("project:{}", proj_id));
            }
        }
        
        // If robust memory is enabled, classify the content to get rich metadata.
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
                    error!("Failed to classify message: {}. Proceeding with default metadata.", e);
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
                        .map(|s| EmbeddingHead::from_str(&s))
                        .filter_map(Result::ok)
                        .collect::<Vec<_>>();

                    self.generate_and_save_embeddings(&saved_entry, &heads_to_use).await?;
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
            memory_type: Some(self.parse_memory_type(&response.memory_type)),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: None,
            lang: None,
            topics: None,
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
                        .map(|s| EmbeddingHead::from_str(&s))
                        .filter_map(Result::ok)
                        .collect::<Vec<_>>();

                    self.generate_and_save_embeddings(&saved_entry, &heads_to_use).await?;
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
            summary: Some(format!("Summary of previous {} messages", original_message_count)),
            memory_type: Some(MemoryType::Summary),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: Some(false),
            lang: Some("natural".to_string()),
            topics: Some(vec!["summary".to_string()]),
        };

        let saved_entry = self.sqlite_store.save(&entry).await?;
        info!("💾 Saved summary to SQLite");

        if CONFIG.is_robust_memory_enabled() {
            // Use both Semantic and Summary heads for summaries
            let summary_heads = vec![EmbeddingHead::Summary, EmbeddingHead::Semantic];
            self.generate_and_save_embeddings(&saved_entry, &summary_heads).await?;
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
        base_entry: &MemoryEntry,
        heads: &[EmbeddingHead],
    ) -> Result<()> {
        if heads.is_empty() {
            return Ok(());
        }

        let mut all_chunks_to_embed: Vec<String> = Vec::new();
        let mut chunk_metadata: Vec<(EmbeddingHead, usize)> = Vec::new();

        for head in heads {
            let chunks = self.text_chunker.chunk_text(&base_entry.content, head)?;
            for (i, chunk_content) in chunks.into_iter().enumerate() {
                all_chunks_to_embed.push(chunk_content);
                chunk_metadata.push((head.clone(), i));
            }
        }

        if all_chunks_to_embed.is_empty() {
            return Ok(());
        }

        info!(
            "Generating {} embedding chunks across {} heads.",
            all_chunks_to_embed.len(),
            heads.len()
        );

        let embeddings = self.llm_client.get_embeddings(&all_chunks_to_embed).await?;

        for (i, embedding) in embeddings.into_iter().enumerate() {
            let (head, chunk_index) = match chunk_metadata.get(i) {
                Some(meta) => meta,
                None => {
                    warn!("Mismatch between embeddings and metadata count. Skipping extra embedding.");
                    continue;
                }
            };
            
            let chunk_content = &all_chunks_to_embed[i];

            let mut chunk_entry = base_entry.clone();
            chunk_entry.content = chunk_content.clone();
            chunk_entry.embedding = Some(embedding);
            chunk_entry.head = Some(head.to_string());
            
            let tags = chunk_entry.tags.get_or_insert_with(Vec::new);
            tags.push(format!("chunk:{}", chunk_index));

            if let Err(e) = self
                .qdrant_multi_store
                .save(head.clone(), &chunk_entry)
                .await
            {
                warn!("Failed to save chunk to {} collection: {}", head.as_str(), e);
            }
        }

        info!("💾 Saved all embedding chunks to Qdrant multi-store.");
        Ok(())
    }

    /// Evaluate and save response (for compatibility)
    pub async fn evaluate_and_save_response(
        &self,
        session_id: &str,
        response: &ChatResponse,
        project_id: Option<&str>,
    ) -> Result<()> {
        if let Some(proj_id) = project_id {
            debug!("Evaluating response for project: {}", proj_id);
        }
        self.save_assistant_response(session_id, response).await
    }

    /// Get recent context from memory
    pub async fn get_recent_context(
        &self,
        session_id: &str,
        n: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.sqlite_store.load_recent(session_id, n).await
    }

    /// Search for similar memories
    pub async fn search_similar(
        &self,
        session_id: &str,
        embedding: &[f32],
        k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        if CONFIG.is_robust_memory_enabled() {
            self.qdrant_multi_store
                .search(EmbeddingHead::Semantic, session_id, embedding, k)
                .await
        } else {
            self.qdrant_store.semantic_search(session_id, embedding, k).await
        }
    }

    // Helper methods
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

    // Getters
    pub fn sqlite_store(&self) -> &Arc<SqliteMemoryStore> {
        &self.sqlite_store
    }

    pub fn qdrant_store(&self) -> &Arc<QdrantMemoryStore> {
        &self.qdrant_store
    }

    pub fn qdrant_multi_store(&self) -> &Arc<QdrantMultiStore> {
        &self.qdrant_multi_store
    }
}
