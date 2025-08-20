// src/services/memory.rs
// Fixed version - Added project_id parameter to save_user_message
// OPTIMIZATION: Using centralized CONFIG for better performance

use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use tracing::{debug, info};

use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};
use crate::services::chat::ChatResponse;
use crate::llm::client::OpenAIClient;
use crate::config::CONFIG;  // ADD THIS IMPORT

/// Unified memory service that manages both SQLite and Qdrant stores
pub struct MemoryService {
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_store: Arc<QdrantMemoryStore>,
    llm_client: Arc<OpenAIClient>,
}

impl MemoryService {
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self {
            sqlite_store,
            qdrant_store,
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
        // Get default salience from CONFIG
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
            debug!("User message for project: {}", proj_id);
        }

        // Check if we should always embed user messages (from CONFIG)
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

        // Get minimum salience for Qdrant from CONFIG
        if let (Some(salience), Some(_)) = (entry.salience, &entry.embedding) {
            if salience >= CONFIG.min_salience_for_qdrant {
                self.qdrant_store.save(&entry).await?;
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

        // Check if we should always embed assistant messages (from CONFIG)
        // Check minimum character count for embedding (from CONFIG)
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

        // Get minimum salience for Qdrant from CONFIG
        if let (Some(salience), Some(_)) = (entry.salience, &entry.embedding) {
            if salience >= CONFIG.min_salience_for_qdrant {
                self.qdrant_store.save(&entry).await?;
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
            salience: Some(2.0),  // Low salience for summaries
            tags: Some(vec!["summary".to_string(), "compressed".to_string()]),
            summary: Some(format!("Summary of previous {} messages.", original_message_count)),
            memory_type: Some(MemoryType::Summary),
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

        // Get minimum salience for Qdrant from CONFIG
        if let (Some(salience), Some(_)) = (entry.salience, &entry.embedding) {
            if salience >= CONFIG.min_salience_for_qdrant {
                self.qdrant_store.save(&entry).await?;
            }
        }

        info!("💾 Saved conversation summary to memory stores");
        Ok(())
    }

    /// Evaluate and save a response
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
        self.qdrant_store.semantic_search(session_id, embedding, k).await
    }

    /// Parse memory type from string
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
}
