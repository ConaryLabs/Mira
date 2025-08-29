// src/services/memory.rs
// PHASE 1: Multi-Collection Qdrant Support for GPT-5 Robust Memory
// PHASE 2: Integrated TextChunker for multi-head embedding generation

use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tracing::{debug, info, warn};

use crate::config::CONFIG;
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::{EmbeddingHead, TextChunker};
use crate::memory::qdrant::multi_store::QdrantMultiStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};
use crate::services::chat::ChatResponse;

/// Unified memory service that manages both SQLite and Qdrant stores
/// PHASE 2: Enhanced with text chunking and multi-head embedding generation
pub struct MemoryService {
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_store: Arc<QdrantMemoryStore>,
    qdrant_multi_store: Arc<QdrantMultiStore>,
    llm_client: Arc<OpenAIClient>,
    text_chunker: TextChunker,
}

impl MemoryService {
    /// PHASE 2: Updated constructor
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        let temp_multi_store = Arc::new(QdrantMultiStore::from_single_store(qdrant_store.clone()));
        Self {
            sqlite_store,
            qdrant_store,
            qdrant_multi_store: temp_multi_store,
            llm_client,
            text_chunker: TextChunker::new().expect("Failed to initialize text chunker"),
        }
    }

    /// PHASE 2: Updated constructor with multi-store support
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

    /// Save a user message to memory stores.
    /// PHASE 2: Generates chunked, multi-head embeddings when robust memory is enabled.
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        let mut entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            embedding: None, // Embeddings are now handled later
            salience: Some(CONFIG.min_salience_for_storage),
            tags: Some(vec!["user_message".to_string()]),
            summary: None,
            memory_type: Some(MemoryType::Event),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        if let Some(proj_id) = project_id {
            if let Some(ref mut tags) = entry.tags {
                tags.push(format!("project:{}", proj_id));
            }
        }

        // Always save the raw message to SQLite first
        self.sqlite_store.save(&entry).await?;
        info!("💾 Saved user message to SQLite");

        // Conditionally generate and save embeddings to Qdrant
        if let Some(salience) = entry.salience {
            if salience >= CONFIG.min_salience_for_qdrant {
                if CONFIG.is_robust_memory_enabled() {
                    let heads_to_use = CONFIG
                        .get_embedding_heads()
                        .into_iter()
                        .filter(|h| h != "summary") // Exclude summary head for user messages
                        .map(|s| EmbeddingHead::from_str(&s))
                        .filter_map(Result::ok)
                        .collect::<Vec<_>>();

                    self.generate_and_save_embeddings(&entry, &heads_to_use).await?;
                } else {
                    // Legacy single-embedding path
                    if let Ok(embedding) = self.llm_client.get_embedding(content).await {
                        entry.embedding = Some(embedding);
                        self.qdrant_store.save(&entry).await?;
                        info!("💾 Saved user message to single Qdrant collection");
                    }
                }
            }
        }

        Ok(())
    }

    /// Save an assistant response to memory stores.
    /// PHASE 2: Generates chunked, multi-head embeddings when robust memory is enabled.
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
            embedding: None, // Embeddings are now handled later
            salience: Some(response.salience as f32),
            tags: Some(response.tags.clone()),
            summary: Some(response.summary.clone()),
            memory_type: Some(self.parse_memory_type(&response.memory_type)),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        self.sqlite_store.save(&entry).await?;
        info!("💾 Saved assistant response to SQLite");

        if let Some(salience) = entry.salience {
            if salience >= CONFIG.min_salience_for_qdrant {
                if CONFIG.is_robust_memory_enabled() {
                    let is_summary = response.tags.contains(&"summary".to_string())
                        || response.memory_type.to_lowercase().contains("summary");

                    let heads_to_use = CONFIG
                        .get_embedding_heads()
                        .into_iter()
                        .filter(|h| if h == "summary" { is_summary } else { true }) // Only use summary head if it's a summary
                        .map(|s| EmbeddingHead::from_str(&s))
                        .filter_map(Result::ok)
                        .collect::<Vec<_>>();

                    self.generate_and_save_embeddings(&entry, &heads_to_use).await?;
                } else {
                    // Legacy single-embedding path
                    if let Ok(embedding) = self.llm_client.get_embedding(&response.output).await {
                        entry.embedding = Some(embedding);
                        self.qdrant_store.save(&entry).await?;
                        info!("💾 Saved assistant response to single Qdrant collection");
                    }
                }
            }
        }
        Ok(())
    }

    /// Save a summary to memory stores.
    /// PHASE 2: Now uses the multi-head embedding system.
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
            memory_type: Some(MemoryType::Other),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        self.sqlite_store.save(&entry).await?;
        info!("💾 Saved summary to SQLite");

        if CONFIG.is_robust_memory_enabled() {
            // Use both Semantic and Summary heads for summaries
            let summary_heads = vec![EmbeddingHead::Summary, EmbeddingHead::Semantic];
            self.generate_and_save_embeddings(&entry, &summary_heads).await?;
        } else {
            // Legacy single-embedding path
            if let Ok(embedding) = self.llm_client.get_embedding(summary_content).await {
                let mut entry_with_embedding = entry;
                entry_with_embedding.embedding = Some(embedding);
                self.qdrant_store.save(&entry_with_embedding).await?;
                info!("💾 Saved summary to single Qdrant collection");
            }
        }

        Ok(())
    }

    /// PHASE 2: New method for multi-head chunking, batch embedding, and saving.
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
            "Generating {} embedding chunks across {} heads for message.",
            all_chunks_to_embed.len(),
            heads.len()
        );

        // Corrected: Use get_embeddings for batch processing
        let embeddings = self.llm_client.get_embeddings(&all_chunks_to_embed).await?;

        for (i, embedding) in embeddings.into_iter().enumerate() {
            if i >= chunk_metadata.len() {
                warn!("Mismatch between embeddings and metadata count. Skipping extra embedding.");
                continue;
            }

            let (head, chunk_index): &(EmbeddingHead, usize) = &chunk_metadata[i];
            let chunk_content: &String = &all_chunks_to_embed[i];

            let mut chunk_entry = base_entry.clone();
            chunk_entry.content = chunk_content.clone();
            chunk_entry.embedding = Some(embedding);

            // Corrected: Add metadata to the 'tags' field as there is no 'payload' field
            let tags = chunk_entry.tags.get_or_insert_with(Vec::new);
            tags.push(format!("head:{}", head.as_str()));
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
            // This now uses the unified EmbeddingHead type and will compile
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

