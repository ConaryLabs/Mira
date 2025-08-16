// src/services/memory.rs
// Fixed version - Added project_id parameter to save_user_message

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
        project_id: Option<&str>,  // ADDED: project_id parameter
    ) -> Result<()> {
        let mut entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            embedding: None,  // Will be populated below
            salience: Some(5.0),  // Default salience
            tags: Some(vec!["user_message".to_string()]),
            summary: None,
            memory_type: Some(MemoryType::Event),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        // If project_id is provided, add it to tags
        if let Some(proj_id) = project_id {
            if let Some(ref mut tags) = entry.tags {
                tags.push(format!("project:{}", proj_id));
            }
            debug!("User message for project: {}", proj_id);
        }

        // Generate embedding for the content
        match self.llm_client.get_embedding(content).await {
            Ok(embedding) => {
                entry.embedding = Some(embedding);
            }
            Err(e) => {
                debug!("Failed to generate embedding: {}", e);
                // Continue without embedding
            }
        }

        // Save to SQLite (always)
        self.sqlite_store.save(&entry).await?;

        // Save to Qdrant if salience is high enough and we have an embedding
        if let (Some(salience), Some(_)) = (entry.salience, &entry.embedding) {
            if salience >= 3.0 {
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
            embedding: None,  // Will be populated below
            salience: Some(response.salience as f32),  // Changed from f64 to f32
            tags: Some(response.tags.clone()),
            summary: Some(response.summary.clone()),
            memory_type: Some(self.parse_memory_type(&response.memory_type)),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        // Generate embedding for the content
        match self.llm_client.get_embedding(&response.output).await {
            Ok(embedding) => {
                entry.embedding = Some(embedding);
            }
            Err(e) => {
                debug!("Failed to generate embedding: {}", e);
                // Continue without embedding
            }
        }

        // Save to SQLite (always)
        self.sqlite_store.save(&entry).await?;

        // Save to Qdrant if salience is high enough and we have an embedding
        if let (Some(salience), Some(_)) = (entry.salience, &entry.embedding) {
            if salience >= 3.0 {
                self.qdrant_store.save(&entry).await?;
            }
        }

        info!("💾 Saved assistant response to memory stores");
        Ok(())
    }

    /// Evaluate and save a response
    pub async fn evaluate_and_save_response(
        &self,
        session_id: &str,
        response: &ChatResponse,
        project_id: Option<&str>,
    ) -> Result<()> {
        // For now, just save the response
        // In the future, this could do additional evaluation
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
            _ => MemoryType::Other,
        }
    }
}
