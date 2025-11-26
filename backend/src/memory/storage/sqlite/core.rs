// backend/src/memory/storage/sqlite/core.rs

//! Core SQLite operations for memory storage
//!
//! Contains:
//! - MessageAnalysis: Analysis results for messages
//! - MemoryOperations: CRUD for memory entries
//! - AnalysisOperations: Store/retrieve message analysis
//! - EmbeddingOperations: Track embedding references

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::debug;

use crate::memory::core::types::MemoryEntry;

/// Analysis results for a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAnalysis {
    pub mood: Option<String>,
    pub intensity: Option<f32>,
    pub salience: Option<f32>,
    pub original_salience: Option<f32>,
    pub intent: Option<String>,
    pub topics: Option<Vec<String>>,
    pub summary: Option<String>,
    pub relationship_impact: Option<String>,
    pub analysis_version: Option<String>,
    pub contains_code: Option<bool>,
    pub language: Option<String>,
    pub programming_lang: Option<String>,
    pub routed_to_heads: Option<Vec<String>>,
    pub contains_error: Option<bool>,
    pub error_type: Option<String>,
    pub error_severity: Option<String>,
    pub error_file: Option<String>,
}

impl Default for MessageAnalysis {
    fn default() -> Self {
        Self {
            mood: None,
            intensity: None,
            salience: None,
            original_salience: None,
            intent: None,
            topics: None,
            summary: None,
            relationship_impact: None,
            analysis_version: None,
            contains_code: None,
            language: None,
            programming_lang: None,
            routed_to_heads: None,
            contains_error: None,
            error_type: None,
            error_severity: None,
            error_file: None,
        }
    }
}

/// Operations for memory entries (CRUD)
pub struct MemoryOperations {
    pool: SqlitePool,
}

impl MemoryOperations {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Save a memory entry to the database
    pub async fn save_memory_entry(&self, entry: &MemoryEntry) -> Result<MemoryEntry> {
        let tags_json = entry
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());

        let result = sqlx::query(
            r#"
            INSERT INTO memory_entries (
                session_id, response_id, parent_id, role, content, timestamp, tags
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&entry.session_id)
        .bind(&entry.response_id)
        .bind(entry.parent_id)
        .bind(&entry.role)
        .bind(&entry.content)
        .bind(entry.timestamp.timestamp())
        .bind(&tags_json)
        .execute(&self.pool)
        .await?;

        let mut saved = entry.clone();
        saved.id = Some(result.last_insert_rowid());
        let message_id = saved.id.unwrap();

        debug!("Saved memory entry with id: {:?}", saved.id);

        // Save analysis metadata if present
        if entry.salience.is_some() || entry.mood.is_some() || entry.intent.is_some() {
            let topics_json = entry
                .topics
                .as_ref()
                .map(|t| serde_json::to_string(t).unwrap_or_default());

            sqlx::query(
                r#"
                INSERT INTO message_analysis (
                    message_id, mood, intensity, salience, original_salience,
                    intent, topics, summary, contains_code, programming_lang,
                    contains_error, error_type
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(message_id)
            .bind(&entry.mood)
            .bind(entry.intensity)
            .bind(entry.salience)
            .bind(entry.original_salience)
            .bind(&entry.intent)
            .bind(&topics_json)
            .bind(&entry.summary)
            .bind(entry.contains_code)
            .bind(&entry.programming_lang)
            .bind(entry.contains_error)
            .bind(&entry.error_type)
            .execute(&self.pool)
            .await?;

            debug!("Saved analysis metadata for message_id: {}", message_id);
        }

        Ok(saved)
    }

    /// Load recent memory entries for a session (with analysis metadata)
    pub async fn load_recent_memories(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // Use sqlx::query instead of query_as for flexibility with joined results
        let rows = sqlx::query(
            r#"
            SELECT
                m.id, m.session_id, m.response_id, m.parent_id, m.role, m.content, m.timestamp, m.tags,
                a.mood, a.intensity, a.salience, a.original_salience, a.intent, a.topics,
                a.summary, a.contains_code, a.programming_lang, a.contains_error, a.error_type
            FROM memory_entries m
            LEFT JOIN message_analysis a ON m.id = a.message_id
            WHERE m.session_id = ?
            ORDER BY m.timestamp DESC, m.id DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let entries: Vec<MemoryEntry> = rows
            .into_iter()
            .map(|row| {
                use sqlx::Row;
                let id: i64 = row.get("id");
                let session_id: String = row.get("session_id");
                let response_id: Option<String> = row.get("response_id");
                let parent_id: Option<i64> = row.get("parent_id");
                let role: String = row.get("role");
                let content: String = row.get("content");
                let timestamp_raw: i64 = row.get("timestamp");
                let tags_json: Option<String> = row.get("tags");

                let tags = tags_json.and_then(|t| serde_json::from_str(&t).ok());
                let timestamp = DateTime::from_timestamp(timestamp_raw, 0)
                    .unwrap_or_else(|| Utc::now())
                    .with_timezone(&Utc);

                // Analysis fields from LEFT JOIN
                let mood: Option<String> = row.get("mood");
                let intensity: Option<f64> = row.get("intensity");
                let salience: Option<f64> = row.get("salience");
                let original_salience: Option<f64> = row.get("original_salience");
                let intent: Option<String> = row.get("intent");
                let topics_json: Option<String> = row.get("topics");
                let summary: Option<String> = row.get("summary");
                let contains_code: Option<bool> = row.get("contains_code");
                let programming_lang: Option<String> = row.get("programming_lang");
                let contains_error: Option<bool> = row.get("contains_error");
                let error_type: Option<String> = row.get("error_type");

                let topics = topics_json.and_then(|t| serde_json::from_str(&t).ok());

                MemoryEntry {
                    id: Some(id),
                    session_id,
                    response_id,
                    parent_id,
                    role,
                    content,
                    timestamp,
                    tags,
                    mood,
                    intensity: intensity.map(|v| v as f32),
                    salience: salience.map(|v| v as f32),
                    original_salience: original_salience.map(|v| v as f32),
                    intent,
                    topics,
                    summary,
                    relationship_impact: None,
                    contains_code,
                    language: None,
                    programming_lang,
                    analyzed_at: None,
                    analysis_version: None,
                    routed_to_heads: None,
                    last_recalled: None,
                    recall_count: None,
                    contains_error,
                    error_type,
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
                    embedding: None,
                    embedding_heads: None,
                    qdrant_point_ids: None,
                }
            })
            .collect();

        // Return in reverse chronological order (newest first)
        Ok(entries)
    }

    /// Update metadata for a memory entry
    pub async fn update_memory_metadata(
        &self,
        id: i64,
        entry: &MemoryEntry,
    ) -> Result<MemoryEntry> {
        let tags_json = entry
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());

        sqlx::query(
            r#"
            UPDATE memory_entries
            SET tags = ?, content = ?
            WHERE id = ?
            "#,
        )
        .bind(&tags_json)
        .bind(&entry.content)
        .bind(id)
        .execute(&self.pool)
        .await?;

        let mut updated = entry.clone();
        updated.id = Some(id);

        Ok(updated)
    }

    /// Delete a memory entry
    pub async fn delete_memory_entry(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM memory_entries WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

/// Operations for message analysis
pub struct AnalysisOperations {
    pool: SqlitePool,
}

impl AnalysisOperations {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store analysis for a message
    pub async fn store_analysis(&self, message_id: i64, analysis: &MessageAnalysis) -> Result<()> {
        let topics_json = analysis
            .topics
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());

        sqlx::query(
            r#"
            INSERT INTO message_analysis (
                message_id, mood, intensity, salience, intent, topics,
                summary, relationship_impact, contains_code, programming_lang,
                contains_error, error_type, error_severity, error_file,
                analyzed_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(message_id) DO UPDATE SET
                mood = excluded.mood,
                intensity = excluded.intensity,
                salience = excluded.salience,
                intent = excluded.intent,
                topics = excluded.topics,
                summary = excluded.summary,
                relationship_impact = excluded.relationship_impact,
                contains_code = excluded.contains_code,
                programming_lang = excluded.programming_lang,
                contains_error = excluded.contains_error,
                error_type = excluded.error_type,
                error_severity = excluded.error_severity,
                error_file = excluded.error_file,
                analyzed_at = excluded.analyzed_at
            "#,
        )
        .bind(message_id)
        .bind(&analysis.mood)
        .bind(analysis.intensity)
        .bind(analysis.salience)
        .bind(&analysis.intent)
        .bind(&topics_json)
        .bind(&analysis.summary)
        .bind(&analysis.relationship_impact)
        .bind(analysis.contains_code)
        .bind(&analysis.programming_lang)
        .bind(analysis.contains_error)
        .bind(&analysis.error_type)
        .bind(&analysis.error_severity)
        .bind(&analysis.error_file)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;

        debug!("Stored analysis for message {}", message_id);

        Ok(())
    }

    /// Get analysis for a message
    pub async fn get_analysis(&self, message_id: i64) -> Result<Option<MessageAnalysis>> {
        let row = sqlx::query_as::<_, (
            Option<String>, Option<f32>, Option<f32>, Option<String>, Option<String>,
            Option<String>, Option<String>, Option<bool>, Option<String>,
            Option<bool>, Option<String>, Option<String>, Option<String>,
        )>(
            r#"
            SELECT mood, intensity, salience, intent, topics,
                   summary, relationship_impact, contains_code, programming_lang,
                   contains_error, error_type, error_severity, error_file
            FROM message_analysis
            WHERE message_id = ?
            "#,
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(
            mood, intensity, salience, intent, topics_json,
            summary, relationship_impact, contains_code, programming_lang,
            contains_error, error_type, error_severity, error_file,
        )| {
            let topics = topics_json.and_then(|t| serde_json::from_str(&t).ok());

            MessageAnalysis {
                mood,
                intensity,
                salience,
                original_salience: None,
                intent,
                topics,
                summary,
                relationship_impact,
                analysis_version: None,
                contains_code,
                language: None,
                programming_lang,
                routed_to_heads: None,
                contains_error,
                error_type,
                error_severity,
                error_file,
            }
        }))
    }
}

/// Operations for embedding references
pub struct EmbeddingOperations {
    pool: SqlitePool,
}

impl EmbeddingOperations {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store embedding reference for a message
    pub async fn store_embedding_reference(
        &self,
        message_id: i64,
        embedding_heads: &[String],
    ) -> Result<()> {
        let heads_json = serde_json::to_string(embedding_heads)?;

        sqlx::query(
            r#"
            INSERT INTO message_embeddings (message_id, embedding_heads, created_at)
            VALUES (?, ?, ?)
            ON CONFLICT(message_id) DO UPDATE SET
                embedding_heads = excluded.embedding_heads,
                created_at = excluded.created_at
            "#,
        )
        .bind(message_id)
        .bind(&heads_json)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;

        debug!(
            "Stored embedding reference for message {} with heads: {:?}",
            message_id, embedding_heads
        );

        Ok(())
    }

    /// Get embedding heads for a message
    pub async fn get_embedding_heads(&self, message_id: i64) -> Result<Option<Vec<String>>> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT embedding_heads FROM message_embeddings WHERE message_id = ?",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.and_then(|(heads_json,)| serde_json::from_str(&heads_json).ok()))
    }
}
