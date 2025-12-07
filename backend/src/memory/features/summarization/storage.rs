// backend/src/memory/features/summarization/storage.rs

//! Storage layer for summaries
//!
//! Handles storing summaries in both SQLite (structured) and Qdrant (vector search).

use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tracing::{debug, warn};

use crate::llm::embeddings::EmbeddingHead;
use crate::llm::provider::EmbeddingProvider;
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::memory_types::{SummaryRecord, SummaryType};
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;

/// Storage layer for summaries - handles both SQLite and Qdrant persistence
pub struct SummaryStorage {
    embedding_client: Arc<dyn EmbeddingProvider>,
    sqlite_store: Arc<SqliteMemoryStore>,
    multi_store: Arc<QdrantMultiStore>,
}

impl SummaryStorage {
    /// Create new summary storage with all required dependencies
    pub fn new(
        embedding_client: Arc<dyn EmbeddingProvider>,
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            embedding_client,
            sqlite_store,
            multi_store,
        }
    }

    /// Store a summary in both SQLite and Qdrant
    pub async fn store_summary(
        &self,
        session_id: &str,
        summary: &str,
        summary_type: SummaryType,
        message_count: usize,
    ) -> Result<i64> {
        let type_str = match summary_type {
            SummaryType::Rolling => "rolling",
            SummaryType::Snapshot => "snapshot",
        };

        // Store in SQLite via SqliteMemoryStore's pool
        let summary_id = self
            .sqlite_store
            .store_rolling_summary(session_id, type_str, summary, message_count)
            .await?;

        debug!(
            "Stored {} summary for session {} (id: {})",
            type_str, session_id, summary_id
        );

        // Embed and store in Qdrant for semantic search
        match self.embed_and_store_summary(session_id, summary, summary_id, type_str).await {
            Ok(_) => {
                debug!("Embedded summary {} in Qdrant", summary_id);
            }
            Err(e) => {
                warn!("Failed to embed summary {} in Qdrant: {}", summary_id, e);
                // Don't fail the whole operation if embedding fails
            }
        }

        Ok(summary_id)
    }

    /// Embed a summary and store in Qdrant's summary collection
    async fn embed_and_store_summary(
        &self,
        session_id: &str,
        summary: &str,
        summary_id: i64,
        summary_type: &str,
    ) -> Result<()> {
        // Generate embedding for the summary
        let embedding = self.embedding_client.embed(summary).await?;

        // Create MemoryEntry for Qdrant storage
        let entry = MemoryEntry {
            id: Some(summary_id),
            session_id: session_id.to_string(),
            response_id: None,
            parent_id: None,
            role: "summary".to_string(),
            content: summary.to_string(),
            timestamp: Utc::now(),
            tags: Some(vec![
                format!("summary_type:{}", summary_type),
                format!("session:{}", session_id),
            ]),
            mood: None,
            intensity: None,
            salience: Some(1.0), // Summaries are always high salience
            original_salience: None,
            intent: None,
            topics: None,
            summary: None,
            relationship_impact: None,
            contains_code: None,
            language: None,
            programming_lang: None,
            analyzed_at: None,
            analysis_version: None,
            routed_to_heads: None,
            last_recalled: None,
            recall_count: None,
            contains_error: None,
            error_type: None,
            error_severity: None,
            error_file: None,
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
            embedding: Some(embedding),
            embedding_heads: Some(vec!["conversation".to_string()]),
            qdrant_point_ids: None,
        };

        // Store in conversation collection (summaries are part of conversation data)
        self.multi_store.save(EmbeddingHead::Conversation, &entry).await?;

        Ok(())
    }

    /// Get the latest summaries for a session
    pub async fn get_latest_summaries(&self, session_id: &str) -> Result<Vec<SummaryRecord>> {
        self.sqlite_store.get_rolling_summaries(session_id).await
    }

    /// Search summaries by semantic similarity
    pub async fn search_summaries(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let embedding = self.embedding_client.embed(query).await?;
        self.multi_store
            .search(EmbeddingHead::Conversation, session_id, &embedding, limit)
            .await
    }
}
