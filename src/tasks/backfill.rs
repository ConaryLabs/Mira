// src/tasks/backfill.rs

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::{
    AppState,
    config::CONFIG,
    llm::embeddings::EmbeddingHead,
    memory::core::types::MemoryEntry,
    memory::storage::qdrant::multi_store::QdrantMultiStore,
};
use chrono::{DateTime, Utc};

pub struct BackfillTask {
    app_state: Arc<AppState>,
}

impl BackfillTask {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }

    /// Run the backfill task once on startup
    pub async fn run(&self) -> Result<()> {
        info!("Starting embedding backfill task");

        let pool = &self.app_state.sqlite_store.pool;
        let embedding_client = &self.app_state.embedding_client;
        let multi_store = &self.app_state.memory_service.get_multi_store();

        // Find all messages that need embeddings
        let messages = self.find_messages_needing_embeddings(pool).await?;

        if messages.is_empty() {
            info!("No messages need embedding backfill");
            return Ok(());
        }

        info!("Found {} messages needing embeddings", messages.len());

        let mut total_processed = 0;
        let mut total_embeddings_stored = 0;
        let mut total_errors = 0;

        // Process in batches of 100
        const BATCH_SIZE: usize = 100;
        for chunk in messages.chunks(BATCH_SIZE) {
            match self.process_batch(
                pool,
                chunk,
                embedding_client,
                multi_store,
            ).await {
                Ok(stored_count) => {
                    total_processed += chunk.len();
                    total_embeddings_stored += stored_count;
                    info!(
                        "Backfill progress: {}/{} messages processed, {} embeddings stored",
                        total_processed, messages.len(), total_embeddings_stored
                    );
                }
                Err(e) => {
                    error!("Failed to process batch: {}", e);
                    total_errors += 1;
                }
            }
        }

        info!(
            "Embedding backfill complete: {} messages processed, {} embeddings stored, {} errors",
            total_processed, total_embeddings_stored, total_errors
        );

        Ok(())
    }

    /// Find all messages that have routed_to_heads but no embeddings in Qdrant
    async fn find_messages_needing_embeddings(&self, pool: &SqlitePool) -> Result<Vec<MessageForBackfill>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                me.id,
                me.session_id,
                me.content,
                me.timestamp,
                ma.salience,
                ma.routed_to_heads,
                ma.topics,
                ma.mood,
                ma.intensity,
                ma.intent,
                ma.summary,
                ma.relationship_impact,
                ma.contains_code,
                ma.language,
                ma.programming_lang
            FROM memory_entries me
            JOIN message_analysis ma ON me.id = ma.message_id
            WHERE ma.routed_to_heads IS NOT NULL 
              AND ma.routed_to_heads != '[]'
              AND ma.salience >= ?
            ORDER BY me.id ASC
            "#,
            CONFIG.salience_min_for_embed
        )
        .fetch_all(pool)
        .await?;

        let mut messages = Vec::new();
        for row in rows {
            // Parse routed_to_heads JSON
            let routed_to_heads: Vec<String> = match serde_json::from_str(&row.routed_to_heads.unwrap_or_else(|| "[]".to_string())) {
                Ok(heads) => heads,
                Err(e) => {
                    warn!("Failed to parse routed_to_heads for message {}: {}", row.id, e);
                    continue;
                }
            };

            if routed_to_heads.is_empty() {
                continue;
            }

            // Parse topics JSON
            let topics: Vec<String> = match serde_json::from_str(&row.topics.unwrap_or_else(|| "[]".to_string())) {
                Ok(t) => t,
                Err(_) => vec!["general".to_string()],
            };

            messages.push(MessageForBackfill {
                id: row.id,
                session_id: row.session_id,
                content: row.content,
                timestamp: row.timestamp.to_string(), // Convert i64 to String
                salience: row.salience.unwrap_or(0.5) as f32, // Cast f64 to f32
                routed_to_heads,
                topics,
                mood: row.mood,
                intensity: row.intensity.map(|i| i as f32), // Map and cast Option<f64> to Option<f32>
                intent: row.intent,
                summary: row.summary,
                relationship_impact: row.relationship_impact,
                contains_code: row.contains_code.unwrap_or(false),
                language: row.language.unwrap_or_else(|| "en".to_string()),
                programming_lang: row.programming_lang,
            });
        }

        Ok(messages)
    }

    /// Process a batch of messages - generate embeddings and store in Qdrant
    async fn process_batch(
        &self,
        _pool: &SqlitePool,
        messages: &[MessageForBackfill],
        embedding_client: &crate::llm::client::OpenAIClient,
        multi_store: &Arc<QdrantMultiStore>,
    ) -> Result<usize> {
        // Collect all texts for batch embedding
        let texts: Vec<String> = messages.iter().map(|m| m.content.clone()).collect();

        // Get batch embeddings from OpenAI
        let embeddings = match embedding_client.embedding_client().get_batch_embeddings(texts).await {
            Ok(embs) => embs,
            Err(e) => {
                error!("Failed to get batch embeddings: {}", e);
                return Err(e);
            }
        };

        if embeddings.len() != messages.len() {
            error!(
                "Embedding count mismatch: got {} embeddings for {} messages",
                embeddings.len(),
                messages.len()
            );
            return Ok(0);
        }

        let mut stored_count = 0;

        // Store each message's embedding in the appropriate collections
        for (message, embedding) in messages.iter().zip(embeddings.iter()) {
            for head_str in &message.routed_to_heads {
                // Parse the head string to EmbeddingHead enum
                let head = match head_str.parse::<EmbeddingHead>() {
                    Ok(h) => h,
                    Err(e) => {
                        warn!("Invalid embedding head '{}' for message {}: {}", head_str, message.id, e);
                        continue;
                    }
                };

                // Check if this head is enabled in config
                if !CONFIG.embed_heads.contains(head_str) {
                    continue;
                }

                // Parse timestamp string to DateTime<Utc>
                let timestamp = match message.timestamp.parse::<i64>() {
                    Ok(ts) => DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now),
                    Err(_) => Utc::now(),
                };

                // Create MemoryEntry for Qdrant
                let memory_entry = MemoryEntry {
                    id: Some(message.id),
                    session_id: message.session_id.clone(),
                    response_id: None,
                    parent_id: None,
                    role: "assistant".to_string(),
                    content: message.content.clone(),
                    timestamp,
                    tags: Some(message.topics.clone()),
                    mood: message.mood.clone(),
                    intensity: message.intensity,
                    salience: Some(message.salience),
                    original_salience: Some(message.salience),
                    intent: message.intent.clone(),
                    topics: Some(message.topics.clone()),
                    summary: message.summary.clone(),
                    relationship_impact: message.relationship_impact.clone(),
                    contains_code: Some(message.contains_code),
                    language: Some(message.language.clone()),
                    programming_lang: message.programming_lang.clone(),
                    analyzed_at: Some(Utc::now()),
                    analysis_version: Some("backfill_v1".to_string()),
                    routed_to_heads: Some(message.routed_to_heads.clone()),
                    last_recalled: Some(Utc::now()),
                    recall_count: Some(0),
                    model_version: None,
                    prompt_tokens: None,
                    completion_tokens: None,
                    reasoning_tokens: None,
                    total_tokens: None,
                    latency_ms: None,
                    generation_time_ms: None,
                    finish_reason: None,
                    tool_calls: None,
                    temperature: None,
                    max_tokens: None,
                    embedding: Some(embedding.clone()),
                    embedding_heads: Some(message.routed_to_heads.clone()),
                    qdrant_point_ids: None,
                };

                // Store in Qdrant collection
                match multi_store.save(head, &memory_entry).await {
                    Ok(_) => {
                        stored_count += 1;
                    }
                    Err(e) => {
                        warn!(
                            "Failed to store embedding for message {} in {} collection: {}",
                            message.id,
                            head.as_str(),
                            e
                        );
                    }
                }
            }
        }

        Ok(stored_count)
    }
}

#[derive(Debug, Clone)]
struct MessageForBackfill {
    id: i64,
    session_id: String,
    content: String,
    timestamp: String,
    salience: f32,
    routed_to_heads: Vec<String>,
    topics: Vec<String>,
    mood: Option<String>,
    intensity: Option<f32>,
    intent: Option<String>,
    summary: Option<String>,
    relationship_impact: Option<String>,
    contains_code: bool,
    language: String,
    programming_lang: Option<String>,
}
