//! Implements MemoryStore for SQLite (session/recency memory).

use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryTag, MemoryType};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde_json;
use sqlx::{Row, SqlitePool};

pub struct SqliteMemoryStore {
    pub pool: SqlitePool, // Make pool public so handlers can access it
}

impl SqliteMemoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // Helper to convert Vec<f32> to Vec<u8> for BLOB storage
    fn embedding_to_blob(embedding: &Option<Vec<f32>>) -> Option<Vec<u8>> {
        embedding.as_ref().map(|vec| {
            vec.iter()
                .flat_map(|f| f.to_le_bytes())
                .collect::<Vec<u8>>()
        })
    }

    // Helper to convert BLOB (Vec<u8>) to Vec<f32>
    fn blob_to_embedding(blob: Option<Vec<u8>>) -> Option<Vec<f32>> {
        blob.map(|bytes| {
            bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
                .collect()
        })
    }
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    /// **MODIFIED**: Now returns the saved MemoryEntry with its new database ID.
    async fn save(&self, entry: &MemoryEntry) -> Result<MemoryEntry> {
        let tags_json = entry
            .tags
            .as_ref()
            .map(|tags| serde_json::to_string(tags).unwrap_or("[]".to_string()));

        let memory_type_str = entry.memory_type.as_ref().map(|mt| format!("{:?}", mt));
        let logprobs_json = entry
            .logprobs
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or("null".to_string()));

        let row = sqlx::query(
            r#"
            INSERT INTO chat_history (
                session_id, role, content, timestamp,
                embedding, salience, tags, summary, memory_type,
                logprobs, moderation_flag, system_fingerprint
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(&entry.session_id)
        .bind(&entry.role)
        .bind(&entry.content)
        .bind(entry.timestamp.naive_utc())
        .bind(Self::embedding_to_blob(&entry.embedding))
        .bind(entry.salience)
        .bind(tags_json)
        .bind(&entry.summary)
        .bind(memory_type_str)
        .bind(logprobs_json)
        .bind(entry.moderation_flag)
        .bind(&entry.system_fingerprint)
        .fetch_one(&self.pool)
        .await?;

        let new_id: i64 = row.get("id");
        let mut saved_entry = entry.clone();
        saved_entry.id = Some(new_id);

        Ok(saved_entry)
    }

    async fn load_recent(&self, session_id: &str, n: usize) -> Result<Vec<MemoryEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT id, session_id, role, content, timestamp, embedding, salience, tags, summary, memory_type,
                   logprobs, moderation_flag, system_fingerprint
            FROM chat_history
            WHERE session_id = ?
            ORDER BY timestamp DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(n as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut entries = Vec::new();

        for row in rows {
            let id: i64 = row.get("id");
            let session_id: String = row.get("session_id");
            let role: String = row.get("role");
            let content: String = row.get("content");
            let timestamp: NaiveDateTime = row.get("timestamp");
            let embedding = Self::blob_to_embedding(row.get("embedding"));
            let salience: Option<f32> = row.get("salience");
            let tags: Option<String> = row.get("tags");
            let summary: Option<String> = row.get("summary");
            let memory_type: Option<String> = row.get("memory_type");
            let logprobs: Option<String> = row.get("logprobs");
            let moderation_flag: Option<bool> = row.get("moderation_flag");
            let system_fingerprint: Option<String> = row.get("system_fingerprint");

            let tags_vec = tags
                .as_ref()
                .and_then(|s| serde_json::from_str::<Vec<MemoryTag>>(s).ok());
            let logprobs_val = logprobs
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok());

            let memory_type_enum = memory_type.as_ref().and_then(|mt| match mt.as_str() {
                "Feeling" => Some(MemoryType::Feeling),
                "Fact" => Some(MemoryType::Fact),
                "Joke" => Some(MemoryType::Joke),
                "Promise" => Some(MemoryType::Promise),
                "Event" => Some(MemoryType::Event),
                "Summary" => Some(MemoryType::Summary),
                _ => Some(MemoryType::Other),
            });

            entries.push(MemoryEntry {
                id: Some(id),
                session_id,
                role,
                content,
                timestamp: Utc.from_utc_datetime(&timestamp),
                embedding,
                salience,
                tags: tags_vec,
                summary,
                memory_type: memory_type_enum,
                logprobs: logprobs_val,
                moderation_flag,
                system_fingerprint,
                // **MODIFIED**: Add default values for new fields
                head: None,
                is_code: None,
                lang: None,
                topics: None,
            });
        }

        Ok(entries)
    }

    async fn semantic_search(
        &self,
        _session_id: &str,
        _embedding: &[f32],
        _k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // SQLite does not support semantic search. Return empty.
        Ok(Vec::new())
    }

    /// **MODIFIED**: Now returns the updated MemoryEntry.
    async fn update_metadata(&self, id: i64, updated: &MemoryEntry) -> Result<MemoryEntry> {
        let tags_json = updated
            .tags
            .as_ref()
            .map(|tags| serde_json::to_string(tags).unwrap_or("[]".to_string()));

        let memory_type_str = updated.memory_type.as_ref().map(|mt| format!("{:?}", mt));
        let logprobs_json = updated
            .logprobs
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or("null".to_string()));

        sqlx::query(
            r#"
            UPDATE chat_history
            SET embedding = ?, salience = ?, tags = ?, summary = ?, memory_type = ?,
                logprobs = ?, moderation_flag = ?, system_fingerprint = ?
            WHERE id = ?
            "#,
        )
        .bind(Self::embedding_to_blob(&updated.embedding))
        .bind(updated.salience)
        .bind(tags_json)
        .bind(&updated.summary)
        .bind(memory_type_str)
        .bind(logprobs_json)
        .bind(updated.moderation_flag)
        .bind(&updated.system_fingerprint)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(updated.clone())
    }

    async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM chat_history WHERE id = ?
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
