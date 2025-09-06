// src/memory/sqlite/store.rs
// SQLite-backed memory store implementing the MemoryStore trait
// with additional methods for WebSocket operations

use crate::memory::sqlite::migration;
use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryTag, MemoryType};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde_json;
use sqlx::{Row, SqlitePool};

pub struct SqliteMemoryStore {
    pub pool: SqlitePool,
}

impl SqliteMemoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Runs all required database migrations
    pub async fn run_migrations(&self) -> Result<()> {
        migration::run_migrations(&self.pool).await
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

    #[inline]
    fn bool_to_sqlite(v: Option<bool>) -> i64 {
        v.unwrap_or(false) as i64
    }
    
    // New methods for WebSocket support
    
    /// Update the pin status of a memory
    pub async fn update_pin_status(&self, memory_id: i64, pinned: bool) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE chat_history 
            SET pinned = ?, last_accessed = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(pinned as i64)
        .bind(memory_id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// Get a memory by its ID
    pub async fn get_by_id(&self, memory_id: i64) -> Result<Option<MemoryEntry>> {
        let row = sqlx::query(
            r#"
            SELECT id, session_id, role, content, timestamp, embedding, salience, tags, summary, memory_type,
                   logprobs, moderation_flag, system_fingerprint,
                   pinned, subject_tag, last_accessed
            FROM chat_history
            WHERE id = ?
            "#,
        )
        .bind(memory_id)
        .fetch_optional(&self.pool)
        .await?;
        
        let entry = row.map(|r| {
            let id: i64 = r.get("id");
            let session_id: String = r.get("session_id");
            let role: String = r.get("role");
            let content: String = r.get("content");
            let timestamp: NaiveDateTime = r.get("timestamp");
            let embedding = Self::blob_to_embedding(r.get("embedding"));
            let salience: Option<f32> = r.get("salience");
            let tags: Option<String> = r.get("tags");
            let summary: Option<String> = r.get("summary");
            let memory_type: Option<String> = r.get("memory_type");
            let logprobs: Option<String> = r.get("logprobs");
            let moderation_flag: Option<bool> = r.get("moderation_flag");
            let system_fingerprint: Option<String> = r.get("system_fingerprint");
            
            let pinned_i: Option<i64> = r.get("pinned");
            let subject_tag: Option<String> = r.get("subject_tag");
            let last_accessed: Option<NaiveDateTime> = r.get("last_accessed");
            
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
            
            MemoryEntry {
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
                head: None,
                is_code: None,
                lang: None,
                topics: None,
                pinned: Some(pinned_i.unwrap_or(0) != 0),
                subject_tag,
                last_accessed: last_accessed.map(|naive| Utc.from_utc_datetime(&naive)),
            }
        });
        
        Ok(entry)
    }
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
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
                logprobs, moderation_flag, system_fingerprint,
                pinned, subject_tag, last_accessed
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(Self::bool_to_sqlite(entry.pinned))
        .bind(&entry.subject_tag)
        .bind(entry.last_accessed.map(|t| t.naive_utc()))
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
                   logprobs, moderation_flag, system_fingerprint,
                   pinned, subject_tag, last_accessed
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

        let mut entries = Vec::with_capacity(rows.len());
        let mut ids: Vec<i64> = Vec::with_capacity(rows.len());

        for row in rows {
            let id: i64 = row.get("id");
            ids.push(id);

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

            let pinned_i: Option<i64> = row.get("pinned");
            let subject_tag: Option<String> = row.get("subject_tag");
            let last_accessed: Option<NaiveDateTime> = row.get("last_accessed");

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
                head: None,
                is_code: None,
                lang: None,
                topics: None,
                pinned: Some(pinned_i.unwrap_or(0) != 0),
                subject_tag,
                last_accessed: last_accessed.map(|naive| Utc.from_utc_datetime(&naive)),
            });
        }

        // Update last_accessed for all retrieved entries
        for id in ids {
            let _ = sqlx::query(
                r#"
                UPDATE chat_history
                SET last_accessed = CURRENT_TIMESTAMP
                WHERE id = ?
                "#,
            )
            .bind(id)
            .execute(&self.pool)
            .await;
        }

        // Reverse to get chronological order (oldest first)
        entries.reverse();
        
        Ok(entries)
    }

    async fn semantic_search(
        &self,
        _session_id: &str,
        _embedding: &[f32],
        _k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // SQLite doesn't do semantic search - that's Qdrant's job
        Ok(Vec::new())
    }

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
                logprobs = ?, moderation_flag = ?, system_fingerprint = ?,
                pinned = ?, subject_tag = ?, last_accessed = COALESCE(?, last_accessed)
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
        .bind(Self::bool_to_sqlite(updated.pinned))
        .bind(&updated.subject_tag)
        .bind(updated.last_accessed.map(|t| t.naive_utc()))
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(updated.clone())
    }

    async fn delete(&self, id: i64) -> Result<()> {
        let rows_affected = sqlx::query(
            r#"
            DELETE FROM chat_history WHERE id = ?
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        
        if rows_affected == 0 {
            return Err(anyhow::anyhow!("Memory with id {} not found", id));
        }
        
        Ok(())
    }
}
