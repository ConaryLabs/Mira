// src/services/memory.rs
// Phase 7: Unified memory service integrating SQLite and Qdrant

use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use tracing::{info, warn};

use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};
use crate::llm::client::OpenAIClient;
use crate::services::chat::ChatResponse;

/// Unified memory service handling both SQLite and Qdrant stores
pub struct MemoryService {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_store: Arc<QdrantMemoryStore>,
    pub llm_client: Arc<OpenAIClient>,
}

impl MemoryService {
    /// Create a new MemoryService with both stores
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

    /// Save a user message to memory
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
    ) -> Result<()> {
        info!("💾 Saving user message for session: {}", session_id);

        let mut embedding = None;

        // Try to get embedding for the message
        if content.len() > 10 {
            match self.llm_client.get_embedding(content).await {
                Ok(vec) => {
                    info!("✅ Generated embedding for user message");
                    embedding = Some(vec);
                }
                Err(e) => warn!("⚠️ Failed to get embedding: {:?}", e),
            }
        }

        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            embedding,
            salience: Some(5.0), // Default salience for user messages
            tags: Some(vec!["user_input".to_string()]),
            summary: None,
            memory_type: Some(MemoryType::Other),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        // Save to SQLite (primary store)
        self.sqlite_store.save(&entry).await?;

        // Save to Qdrant if we have an embedding and it's somewhat salient
        if let Some(ref _emb) = entry.embedding {
            if entry.salience.unwrap_or(0.0) >= 3.0 {
                match self.qdrant_store.save(&entry).await {
                    Ok(_) => info!("✅ Saved to Qdrant"),
                    Err(e) => warn!("⚠️ Failed to save to Qdrant: {:?}", e),
                }
            }
        }

        Ok(())
    }

    /// Save an assistant response to memory
    pub async fn save_assistant_response(
        &self,
        session_id: &str,
        response: &ChatResponse,
    ) -> Result<()> {
        info!("💾 Saving assistant response for session: {}", session_id);

        let mut embedding = None;

        // Try to get embedding for the response
        if response.output.len() > 10 {
            match self.llm_client.get_embedding(&response.output).await {
                Ok(vec) => {
                    info!("✅ Generated embedding for assistant response");
                    embedding = Some(vec);
                }
                Err(e) => warn!("⚠️ Failed to get embedding: {:?}", e),
            }
        }

        let memory_type = match response.memory_type.to_lowercase().as_str() {
            "feeling" => MemoryType::Feeling,
            "fact" => MemoryType::Fact,
            "joke" => MemoryType::Joke,
            "promise" => MemoryType::Promise,
            "event" => MemoryType::Event,
            _ => MemoryType::Other,
        };

        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "assistant".to_string(),
            content: response.output.clone(),
            timestamp: Utc::now(),
            embedding,
            salience: Some(response.salience as f32),
            tags: Some(response.tags.clone()),
            summary: Some(response.summary.clone()),
            memory_type: Some(memory_type),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        // Save to SQLite (primary store)
        self.sqlite_store.save(&entry).await?;

        // Save to Qdrant if we have an embedding and it's salient enough
        if let Some(ref _emb) = entry.embedding {
            if entry.salience.unwrap_or(0.0) >= 3.0 {
                match self.qdrant_store.save(&entry).await {
                    Ok(_) => info!("✅ Saved to Qdrant"),
                    Err(e) => warn!("⚠️ Failed to save to Qdrant: {:?}", e),
                }
            }
        }

        Ok(())
    }

    /// Get recent conversation context for a session
    pub async fn get_recent_context(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        info!("📖 Retrieving recent context for session: {}", session_id);

        // Use the trait method properly
        let recent = self.sqlite_store
            .load_recent(session_id, limit)
            .await?;

        // Filter out memories with embeddings for similarity scoring
        let with_embeddings: Vec<_> = recent
            .iter()
            .filter(|msg| msg.embedding.is_some())
            .collect();

        if !with_embeddings.is_empty() {
            info!(
                "Found {} messages with embeddings for context",
                with_embeddings.len()
            );
        }

        Ok(recent)
    }

    /// Search for similar memories using vector similarity
    pub async fn search_similar(
        &self,
        session_id: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        info!("🔍 Searching similar memories for session: {}", session_id);

        // Search in Qdrant for similar vectors
        match self.qdrant_store
            .semantic_search(session_id, embedding, limit)
            .await
        {
            Ok(results) => {
                info!("Found {} similar memories", results.len());
                Ok(results)
            }
            Err(e) => {
                warn!("⚠️ Vector search failed, falling back to recent: {:?}", e);
                // Fallback to recent messages if vector search fails
                self.get_recent_context(session_id, limit).await
            }
        }
    }

    /// Get conversation statistics for a session
    pub async fn get_stats(&self, session_id: &str) -> Result<ConversationStats> {
        let total_messages = self.sqlite_store
            .load_recent(session_id, 1000)
            .await?
            .len();

        let recent_messages = self.sqlite_store
            .load_recent(session_id, 10)
            .await?;

        let mut mood_counts = std::collections::HashMap::new();
        let mut total_salience = 0.0;
        let mut salient_count = 0;

        for msg in &recent_messages {
            if let Some(salience) = msg.salience {
                total_salience += salience;
                salient_count += 1;
            }

            if let Some(tags) = &msg.tags {
                for tag in tags {
                    *mood_counts.entry(tag.clone()).or_insert(0) += 1;
                }
            }
        }

        let avg_salience = if salient_count > 0 {
            total_salience / salient_count as f32
        } else {
            5.0
        };

        Ok(ConversationStats {
            total_messages,
            recent_count: recent_messages.len(),
            average_salience: avg_salience,
            dominant_tags: mood_counts
                .into_iter()
                .map(|(k, v)| (k, v))
                .collect(),
        })
    }
    
    /// Get recent messages (compatibility method)
    pub async fn get_recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.get_recent_context(session_id, limit).await
    }
    
    /// Evaluate and save a response (for document imports)
    pub async fn evaluate_and_save_response(
        &self,
        session_id: &str,
        response: &ChatResponse,
        _project_id: Option<&str>,
    ) -> Result<()> {
        // Just save the response using our existing method
        self.save_assistant_response(session_id, response).await
    }
}

/// Statistics about a conversation
#[derive(Debug, Clone)]
pub struct ConversationStats {
    pub total_messages: usize,
    pub recent_count: usize,
    pub average_salience: f32,
    pub dominant_tags: Vec<(String, usize)>,
}
