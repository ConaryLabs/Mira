// src/memory/storage/sqlite/core/memory_operations.rs

use crate::memory::core::types::MemoryEntry;
use anyhow::Result;
use chrono::{TimeZone, NaiveDateTime, Utc};
use sqlx::{Row, SqlitePool};
use tracing::debug;

/// Handles basic memory storage operations (MemoryStore trait implementation)
pub struct MemoryOperations {
    pool: SqlitePool,
}

impl MemoryOperations {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Core MemoryStore::save implementation
    pub async fn save_memory_entry(&self, entry: &MemoryEntry) -> Result<MemoryEntry> {
        let tags_json = entry
            .tags
            .as_ref()
            .map(|tags| serde_json::to_string(tags).unwrap_or("[]".to_string()));

        // Generate a response_id for tracking conversation threads
        let response_id = match &entry.role[..] {
            "assistant" => Some(uuid::Uuid::new_v4().to_string()),
            _ => entry.response_id.clone(),
        };

        // Find parent_id by getting the most recent message in this session
        let parent_id: Option<i64> = if entry.role != "system" {
            sqlx::query_scalar(
                r#"
                SELECT id FROM memory_entries 
                WHERE session_id = ? 
                ORDER BY timestamp DESC 
                LIMIT 1
                "#
            )
            .bind(&entry.session_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            None
        };

        let row = sqlx::query(
            r#"
            INSERT INTO memory_entries (
                session_id, response_id, parent_id, role, content, timestamp, tags
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(&entry.session_id)
        .bind(&response_id)
        .bind(parent_id)
        .bind(&entry.role)
        .bind(&entry.content)
        .bind(entry.timestamp.naive_utc())
        .bind(tags_json)
        .fetch_one(&self.pool)
        .await?;

        let new_id: i64 = row.get("id");
        let mut saved_entry = entry.clone();
        saved_entry.id = Some(new_id);
        saved_entry.response_id = response_id.clone();

        debug!("Saved memory entry {} for session {} (parent: {:?})", 
               new_id, entry.session_id, parent_id);
        Ok(saved_entry)
    }

    /// Core MemoryStore::load_recent implementation
    /// FIXED: Now joins with message_analysis to load all analysis metadata
    pub async fn load_recent_memories(&self, session_id: &str, n: usize) -> Result<Vec<MemoryEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                m.id, m.session_id, m.role, m.content, m.timestamp, m.tags, m.response_id, m.parent_id,
                a.mood, a.intensity, a.salience, a.original_salience, a.intent, a.topics,
                a.summary, a.relationship_impact, a.contains_code, a.language, a.programming_lang,
                a.contains_error, a.error_type, a.error_severity, a.error_file,
                a.analyzed_at, a.analysis_version, a.routed_to_heads, a.last_recalled, a.recall_count
            FROM memory_entries m
            LEFT JOIN message_analysis a ON m.id = a.message_id
            WHERE m.session_id = ?
            ORDER BY m.timestamp DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(n as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut entries = Vec::with_capacity(rows.len());

        for row in rows {
            // Core fields
            let id: i64 = row.get("id");
            let session_id: String = row.get("session_id");
            let role: String = row.get("role");
            let content: String = row.get("content");
            let timestamp: NaiveDateTime = row.get("timestamp");
            let tags: Option<String> = row.get("tags");
            let response_id: Option<String> = row.get("response_id");
            let parent_id: Option<i64> = row.get("parent_id");

            // Parse tags JSON
            let tags_vec = tags
                .as_ref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());

            // Analysis fields from message_analysis table
            let mood: Option<String> = row.get("mood");
            let intensity: Option<f64> = row.get("intensity");
            let salience: Option<f64> = row.get("salience");
            let original_salience: Option<f64> = row.get("original_salience");
            let intent: Option<String> = row.get("intent");
            let topics_json: Option<String> = row.get("topics");
            let summary: Option<String> = row.get("summary");
            let relationship_impact: Option<String> = row.get("relationship_impact");
            let contains_code: Option<bool> = row.get("contains_code");
            let language: Option<String> = row.get("language");
            let programming_lang: Option<String> = row.get("programming_lang");
            let contains_error: Option<bool> = row.get("contains_error");
            let error_type: Option<String> = row.get("error_type");
            let error_severity: Option<String> = row.get("error_severity");
            let error_file: Option<String> = row.get("error_file");
            let analyzed_at: Option<i64> = row.get("analyzed_at");
            let analysis_version: Option<String> = row.get("analysis_version");
            let routed_to_heads_json: Option<String> = row.get("routed_to_heads");
            let last_recalled: Option<i64> = row.get("last_recalled");
            let recall_count: Option<i64> = row.get("recall_count");

            // Parse topics JSON array
            let topics_vec = topics_json
                .as_ref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());

            // Parse routed_to_heads JSON array
            let routed_to_heads_vec = routed_to_heads_json
                .as_ref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());

            // Convert timestamps to DateTime
            let analyzed_at_dt = analyzed_at.map(|ts| Utc.timestamp_opt(ts, 0).single().unwrap());
            let last_recalled_dt = last_recalled.map(|ts| Utc.timestamp_opt(ts, 0).single().unwrap());

            let entry = MemoryEntry {
                id: Some(id),
                session_id,
                response_id,
                parent_id,
                role,
                content,
                timestamp: Utc.from_utc_datetime(&timestamp),
                tags: tags_vec,
                
                // Analysis fields - now properly populated from message_analysis table
                mood,
                intensity: intensity.map(|i| i as f32),
                salience: salience.map(|s| s as f32),
                original_salience: original_salience.map(|s| s as f32),
                intent,
                topics: topics_vec,
                summary,
                relationship_impact,
                contains_code,
                language,
                programming_lang,
                analyzed_at: analyzed_at_dt,
                analysis_version,
                routed_to_heads: routed_to_heads_vec,
                last_recalled: last_recalled_dt,
                recall_count: recall_count,
                
                // Error fields
                contains_error,
                error_type,
                error_severity,
                error_file,
                
                // LLM metadata fields (not stored in message_analysis)
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
                
                // Embedding fields (not stored in message_analysis)
                embedding: None,
                embedding_heads: None,
                qdrant_point_ids: None,
            };

            entries.push(entry);
        }

        debug!("Loaded {} recent memories for session {}", entries.len(), session_id);
        Ok(entries)
    }

    /// Core MemoryStore::update_metadata implementation
    pub async fn update_memory_metadata(&self, id: i64, updated: &MemoryEntry) -> Result<MemoryEntry> {
        let tags_json = updated
            .tags
            .as_ref()
            .map(|tags| serde_json::to_string(tags).unwrap_or("[]".to_string()));

        sqlx::query(
            r#"
            UPDATE memory_entries
            SET tags = ?
            WHERE id = ?
            "#,
        )
        .bind(tags_json)
        .bind(id)
        .execute(&self.pool)
        .await?;

        debug!("Updated metadata for memory entry {}", id);
        Ok(updated.clone())
    }

    /// Core MemoryStore::delete implementation
    pub async fn delete_memory_entry(&self, id: i64) -> Result<()> {
        let rows_affected = sqlx::query(
            r#"
            DELETE FROM memory_entries WHERE id = ?
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        
        if rows_affected == 0 {
            return Err(anyhow::anyhow!("Memory with id {} not found", id));
        }
        
        debug!("Deleted memory entry {}", id);
        Ok(())
    }

    /// Save memory with explicit parent relationship
    pub async fn save_with_parent(&self, entry: &MemoryEntry, parent_id: Option<i64>) -> Result<MemoryEntry> {
        let tags_json = entry
            .tags
            .as_ref()
            .map(|tags| serde_json::to_string(tags).unwrap_or("[]".to_string()));

        // Generate response_id for assistant messages
        let response_id = match &entry.role[..] {
            "assistant" => Some(uuid::Uuid::new_v4().to_string()),
            _ => entry.response_id.clone(),
        };

        let row = sqlx::query(
            r#"
            INSERT INTO memory_entries (
                session_id, response_id, parent_id, role, content, timestamp, tags
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(&entry.session_id)
        .bind(&response_id)
        .bind(parent_id)
        .bind(&entry.role)
        .bind(&entry.content)
        .bind(entry.timestamp.naive_utc())
        .bind(tags_json)
        .fetch_one(&self.pool)
        .await?;

        let new_id: i64 = row.get("id");
        let mut saved_entry = entry.clone();
        saved_entry.id = Some(new_id);
        saved_entry.response_id = response_id;

        debug!("Saved memory entry {} with explicit parent {:?}", new_id, parent_id);
        Ok(saved_entry)
    }
}
