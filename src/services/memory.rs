// src/services/memory.rs
// PHASE 1: Multi-Collection Qdrant Support for GPT-5 Robust Memory
// Updated to use QdrantMultiStore for multi-head embedding storage

use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use tracing::{debug, info, warn};

use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::qdrant::multi_store::{QdrantMultiStore, EmbeddingHead};
use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};
use crate::services::chat::ChatResponse;
use crate::llm::client::OpenAIClient;
use crate::config::CONFIG;

/// Unified memory service that manages both SQLite and Qdrant stores
/// PHASE 1: Enhanced with multi-collection Qdrant support
pub struct MemoryService {
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_store: Arc<QdrantMemoryStore>,
    qdrant_multi_store: Arc<QdrantMultiStore>,
    llm_client: Arc<OpenAIClient>,
}

impl MemoryService {
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        // PHASE 1: Create a temporary multi-store wrapper around the single store
        let temp_multi_store = Arc::new(QdrantMultiStore::from_single_store(qdrant_store.clone()));
        
        Self {
            sqlite_store,
            qdrant_store,
            qdrant_multi_store: temp_multi_store,
            llm_client,
        }
    }
    
    /// PHASE 1: Create with multi-store support
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
        }
    }

    /// Save a user message to memory stores
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        let default_salience = CONFIG.min_salience_for_storage;
        
        let mut entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(default_salience),
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

        if CONFIG.always_embed_user {
            match self.llm_client.get_embedding(content).await {
                Ok(embedding) => {
                    entry.embedding = Some(embedding);
                }
                Err(e) => {
                    debug!("Failed to generate embedding: {}", e);
                }
            }
        }

        self.sqlite_store.save(&entry).await?;

        if let (Some(salience), Some(_)) = (entry.salience, &entry.embedding) {
            if salience >= CONFIG.min_salience_for_qdrant {
                if CONFIG.is_robust_memory_enabled() {
                    info!("💾 Saving user message to multiple Qdrant collections");
                    self.save_to_multi_collections(&entry, &self.get_user_message_heads()).await?;
                } else {
                    self.qdrant_store.save(&entry).await?;
                }
            }
        }

        info!("💾 Saved user message to memory stores");
        Ok(())
    }

    /// Save an assistant response to memory stores
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
        };

        if CONFIG.always_embed_assistant && response.output.len() >= CONFIG.embed_min_chars {
            match self.llm_client.get_embedding(&response.output).await {
                Ok(embedding) => {
                    entry.embedding = Some(embedding);
                }
                Err(e) => {
                    debug!("Failed to generate embedding: {}", e);
                }
            }
        }

        self.sqlite_store.save(&entry).await?;

        if let (Some(salience), Some(_)) = (entry.salience, &entry.embedding) {
            if salience >= CONFIG.min_salience_for_qdrant {
                if CONFIG.is_robust_memory_enabled() {
                    info!("💾 Saving assistant response to multiple Qdrant collections");
                    self.save_to_multi_collections(&entry, &self.get_assistant_response_heads(response)).await?;
                } else {
                    self.qdrant_store.save(&entry).await?;
                }
            }
        }

        info!("💾 Saved assistant response to memory stores");
        Ok(())
    }

    /// Save a summary to memory stores
    pub async fn save_summary(
        &self,
        session_id: &str,
        summary_content: &str,
        original_message_count: usize,
    ) -> Result<()> {
        let mut entry = MemoryEntry {
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

        match self.llm_client.get_embedding(summary_content).await {
            Ok(embedding) => {
                entry.embedding = Some(embedding);
            }
            Err(e) => {
                debug!("Failed to generate embedding for summary: {}", e);
            }
        }

        self.sqlite_store.save(&entry).await?;

        if let Some(_) = &entry.embedding {
            if CONFIG.is_robust_memory_enabled() {
                info!("💾 Saving summary to Summary and Semantic collections");
                let summary_heads = vec![EmbeddingHead::Summary, EmbeddingHead::Semantic];
                self.save_to_multi_collections(&entry, &summary_heads).await?;
            } else {
                self.qdrant_store.save(&entry).await?;
            }
        }

        info!("💾 Saved summary to memory stores");
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
    async fn save_to_multi_collections(
        &self, 
        entry: &MemoryEntry, 
        heads: &[EmbeddingHead]
    ) -> Result<()> {
        for head in heads {
            if let Err(e) = self.qdrant_multi_store.save(*head, entry).await {
                warn!("Failed to save to {} collection: {}", head.as_str(), e);
            }
        }
        Ok(())
    }

    fn get_user_message_heads(&self) -> Vec<EmbeddingHead> {
        let enabled_heads = self.qdrant_multi_store.get_enabled_heads();
        enabled_heads.into_iter()
            .filter(|head| *head != EmbeddingHead::Summary)
            .collect()
    }

    fn get_assistant_response_heads(&self, response: &ChatResponse) -> Vec<EmbeddingHead> {
        let enabled_heads = self.qdrant_multi_store.get_enabled_heads();
        let is_summary = response.tags.contains(&"summary".to_string()) ||
                        response.memory_type.to_lowercase().contains("summary");
        
        enabled_heads.into_iter()
            .filter(|head| {
                if *head == EmbeddingHead::Summary {
                    is_summary
                } else {
                    true
                }
            })
            .collect()
    }

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
