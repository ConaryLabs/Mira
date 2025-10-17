// src/memory/features/summarization/storage/summary_storage.rs

use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use tracing::{info, warn};
use sqlx::Row;
use crate::llm::provider::OpenAiEmbeddings;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::core::types::MemoryEntry;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::features::memory_types::{SummaryType, SummaryRecord};
use crate::config::CONFIG;

/// Handles all summary storage operations
pub struct SummaryStorage {
    embedding_client: Arc<OpenAiEmbeddings>,
    sqlite_store: Arc<SqliteMemoryStore>,
    multi_store: Arc<QdrantMultiStore>,
}

impl SummaryStorage {
    pub fn new(
        embedding_client: Arc<OpenAiEmbeddings>,
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            embedding_client,
            sqlite_store,
            multi_store,
        }
    }

    /// Stores summary in rolling_summaries table + Qdrant
    pub async fn store_summary(
        &self,
        session_id: &str,
        summary: &str,
        summary_type: SummaryType,
        message_count: usize,
    ) -> Result<()> {
        let (first_message_id, last_message_id) = self.get_message_range(session_id, message_count).await?;
        
        let summary_id = self.store_in_rolling_summaries_table(
            session_id,
            summary,
            summary_type,
            message_count,
            first_message_id,
            last_message_id,
        ).await?;
        
        info!("Stored summary {} in rolling_summaries table", summary_id);

        if CONFIG.embed_heads.contains(&"summary".to_string()) {
            match self.embedding_client.embed(summary).await {
                Ok(embedding) => {
                    let qdrant_entry = self.create_qdrant_entry(
                        session_id,
                        summary,
                        summary_type,
                        summary_id,
                        embedding,
                    );
                    
                    // CRITICAL FIX: Capture point_id and track it
                    match self.multi_store
                        .save(EmbeddingHead::Summary, &qdrant_entry)
                        .await {
                        Ok(point_id) => {
                            info!("Stored summary {} embedding in Qdrant Summary collection (point_id: {})", 
                                summary_id, point_id);
                            
                            // Track the embedding in message_embeddings table
                            let collection_name = self.multi_store
                                .get_collection_name(EmbeddingHead::Summary)
                                .unwrap_or_else(|| "memory-summary".to_string());
                            
                            if let Err(e) = self.track_summary_embedding(
                                summary_id,
                                &point_id,
                                &collection_name,
                            ).await {
                                warn!("Failed to track summary {} embedding: {}", summary_id, e);
                            }
                            
                            self.mark_embedding_generated(summary_id).await?;
                        }
                        Err(e) => {
                            warn!("Failed to store summary {} embedding in Qdrant: {}", summary_id, e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to generate embedding for summary {}: {}", summary_id, e);
                }
            }
        }

        Ok(())
    }

    /// Track summary embedding in message_embeddings table
    /// Note: We use message_embeddings table even for summaries since they share the same structure
    async fn track_summary_embedding(
        &self,
        summary_id: i64,
        qdrant_point_id: &str,
        collection_name: &str,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO message_embeddings (
                message_id, qdrant_point_id, collection_name, embedding_head
            ) VALUES (?, ?, ?, 'summary')
            "#,
            summary_id,
            qdrant_point_id,
            collection_name
        )
        .execute(self.sqlite_store.get_pool())
        .await?;
        
        Ok(())
    }

    /// Store summary in the rolling_summaries table
    async fn store_in_rolling_summaries_table(
        &self,
        session_id: &str,
        summary_text: &str,
        summary_type: SummaryType,
        message_count: usize,
        first_message_id: Option<i64>,
        last_message_id: Option<i64>,
    ) -> Result<i64> {
        let summary_type_str = match summary_type {
            SummaryType::Rolling10 => "rolling_10",
            SummaryType::Rolling100 => "rolling_100", 
            SummaryType::Snapshot => "snapshot",
        };

        let row = sqlx::query(
            r#"
            INSERT INTO rolling_summaries (
                session_id, summary_type, summary_text, message_count,
                first_message_id, last_message_id, created_at, embedding_generated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#
        )
        .bind(session_id)
        .bind(summary_type_str)
        .bind(summary_text)
        .bind(message_count as i64)
        .bind(first_message_id)
        .bind(last_message_id)
        .bind(Utc::now().timestamp())
        .bind(false)
        .fetch_one(self.sqlite_store.get_pool())
        .await?;

        let summary_id: i64 = row.get("id");
        Ok(summary_id)
    }

    /// Get the message ID range this summary covers
    async fn get_message_range(&self, session_id: &str, message_count: usize) -> Result<(Option<i64>, Option<i64>)> {
        let rows = sqlx::query(
            r#"
            SELECT id FROM memory_entries 
            WHERE session_id = ? AND role != 'system'
            ORDER BY timestamp DESC 
            LIMIT ?
            "#
        )
        .bind(session_id)
        .bind(message_count as i64)
        .fetch_all(self.sqlite_store.get_pool())
        .await?;

        if rows.is_empty() {
            return Ok((None, None));
        }

        let last_message_id: i64 = rows[0].get("id");
        let first_message_id: i64 = rows[rows.len() - 1].get("id");
        
        Ok((Some(first_message_id), Some(last_message_id)))
    }

    /// Mark embedding as generated in rolling_summaries table
    async fn mark_embedding_generated(&self, summary_id: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE rolling_summaries 
            SET embedding_generated = TRUE 
            WHERE id = ?
            "#
        )
        .bind(summary_id)
        .execute(self.sqlite_store.get_pool())
        .await?;

        Ok(())
    }

    /// Create lightweight entry for Qdrant storage
    fn create_qdrant_entry(
        &self,
        session_id: &str,
        summary: &str,
        summary_type: SummaryType,
        summary_id: i64,
        embedding: Vec<f32>,
    ) -> MemoryEntry {
        let type_tag = match summary_type {
            SummaryType::Rolling10 => "summary:rolling:10",
            SummaryType::Rolling100 => "summary:rolling:100",
            SummaryType::Snapshot => "summary:snapshot",
        };
        
        MemoryEntry {
            id: Some(summary_id),
            session_id: session_id.to_string(),
            response_id: None,
            parent_id: None,
            role: "summary".to_string(),
            content: summary.to_string(),
            timestamp: Utc::now(),
            tags: Some(vec![
                "summary".to_string(),
                type_tag.to_string(),
                "rolling".to_string(),
            ]),
            mood: None,
            intensity: None,
            salience: Some(10.0),
            original_salience: None,
            intent: Some("summarize".to_string()),
            topics: None,
            summary: Some(summary.to_string()),
            relationship_impact: None,
            contains_code: Some(false),
            language: Some("en".to_string()),
            programming_lang: None,
            analyzed_at: Some(Utc::now()),
            analysis_version: Some("summary_v1".to_string()),
            routed_to_heads: Some(vec!["summary".to_string()]),
            last_recalled: Some(Utc::now()),
            recall_count: Some(0),
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
            embedding_heads: Some(vec!["summary".to_string()]),
            qdrant_point_ids: None,
        }
    }

    /// Get summaries for a session
    pub async fn get_summaries_for_session(&self, session_id: &str, limit: usize) -> Result<Vec<SummaryRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, summary_type, summary_text, message_count, created_at 
            FROM rolling_summaries 
            WHERE session_id = ? 
            ORDER BY created_at DESC 
            LIMIT ?
            "#
        )
        .bind(session_id)
        .bind(limit as i64)
        .fetch_all(self.sqlite_store.get_pool())
        .await?;

        let mut summaries = Vec::new();
        for row in rows {
            summaries.push(SummaryRecord {
                id: row.get("id"),
                summary_type: row.get("summary_type"),
                summary_text: row.get("summary_text"),
                message_count: row.get::<i64, _>("message_count") as usize,
                created_at: row.get::<i64, _>("created_at"),
            });
        }

        Ok(summaries)
    }

    /// Get latest summary of each type for context
    /// FIXED: SQLite doesn't support DISTINCT ON - using subquery instead
    pub async fn get_latest_summaries(&self, session_id: &str) -> Result<Vec<SummaryRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, summary_type, summary_text, message_count, created_at
            FROM rolling_summaries 
            WHERE session_id = ? 
              AND id IN (
                SELECT MAX(id)
                FROM rolling_summaries
                WHERE session_id = ?
                GROUP BY summary_type
              )
            ORDER BY created_at DESC
            "#
        )
        .bind(session_id)
        .bind(session_id)
        .fetch_all(self.sqlite_store.get_pool())
        .await?;

        let mut summaries = Vec::new();
        for row in rows {
            summaries.push(SummaryRecord {
                id: row.get("id"),
                summary_type: row.get("summary_type"),
                summary_text: row.get("summary_text"),
                message_count: row.get::<i64, _>("message_count") as usize,
                created_at: row.get::<i64, _>("created_at"),
            });
        }

        Ok(summaries)
    }
}
