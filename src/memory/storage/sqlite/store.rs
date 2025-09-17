// src/memory/storage/sqlite/store.rs
// SQLite-backed memory store implementing the MemoryStore trait
// Correctly matches the new schema with all proper tables

use crate::memory::core::traits::MemoryStore;
use crate::memory::core::types::{MemoryEntry, MemoryTag};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde_json;
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};

pub struct SqliteMemoryStore {
    pub pool: SqlitePool,
}

impl SqliteMemoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Runs all required database migrations (now handled by SQLx)
    pub async fn run_migrations(&self) -> Result<()> {
        // SQLx handles migrations via sqlx migrate run
        info!("Migrations handled by SQLx CLI");
        Ok(())
    }

    // Helper to convert Vec<f32> to Vec<u8> for BLOB storage (kept for compatibility)
    fn embedding_to_blob(embedding: &Option<Vec<f32>>) -> Option<Vec<u8>> {
        embedding.as_ref().map(|vec| {
            vec.iter()
                .flat_map(|f| f.to_le_bytes())
                .collect::<Vec<u8>>()
        })
    }

    // Helper to convert BLOB (Vec<u8>) to Vec<f32> (kept for compatibility)
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
    
    /// Update the pin status of a memory
    pub async fn update_pin_status(&self, memory_id: i64, _pinned: bool) -> Result<()> {
        // Note: pinned column doesn't exist in memory_entries yet
        // This would need to be added to the schema if needed
        debug!("Pin status update requested for memory {} (not implemented)", memory_id);
        Ok(())
    }
    
    /// Get a memory entry with its analysis data
    pub async fn get_by_id(&self, memory_id: i64) -> Result<Option<MemoryEntry>> {
        // Join memory_entries with message_analysis to get full data
        let row = sqlx::query(
            r#"
            SELECT 
                m.id, m.session_id, m.role, m.content, m.timestamp, m.tags,
                m.response_id, m.parent_id,
                a.mood, a.intensity, a.salience, a.intent, a.topics, a.summary,
                a.contains_code, a.programming_lang, a.last_recalled
            FROM memory_entries m
            LEFT JOIN message_analysis a ON m.id = a.message_id
            WHERE m.id = ?
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
            let tags: Option<String> = r.get("tags");
            
            // Analysis fields (may be null if not analyzed yet)
            let salience: Option<f32> = r.get("salience");
            let summary: Option<String> = r.get("summary");
            let topics: Option<String> = r.get("topics");
            let contains_code: Option<bool> = r.get("contains_code");
            let programming_lang: Option<String> = r.get("programming_lang");
            
            let tags_vec = tags
                .as_ref()
                .and_then(|s| serde_json::from_str::<Vec<MemoryTag>>(s).ok());
            
            let topics_vec = topics
                .as_ref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());
            
            MemoryEntry {
                id: Some(id),
                session_id,
                role,
                content,
                timestamp: Utc.from_utc_datetime(&timestamp),
                embedding: None,  // Stored in Qdrant
                salience,
                tags: tags_vec,
                summary,
                memory_type: None,  // Could infer from analysis
                logprobs: None,     // In gpt5_metadata if needed
                moderation_flag: None,
                system_fingerprint: None,
                head: None,
                is_code: contains_code,
                lang: programming_lang,
                topics: topics_vec,
                pinned: None,
                subject_tag: None,
                last_accessed: r.get::<Option<NaiveDateTime>, _>("last_recalled")
                    .map(|dt| Utc.from_utc_datetime(&dt)),
            }
        });
        
        Ok(entry)
    }
    
    /// Get the conversation thread for a response_id
    pub async fn get_thread_by_response(&self, response_id: &str) -> Result<Vec<MemoryEntry>> {
        let rows = sqlx::query(
            r#"
            WITH RECURSIVE thread AS (
                -- Start with the response message
                SELECT id, parent_id, session_id, role, content, timestamp, tags
                FROM memory_entries 
                WHERE response_id = ?
                
                UNION ALL
                
                -- Recursively get parent messages
                SELECT m.id, m.parent_id, m.session_id, m.role, m.content, m.timestamp, m.tags
                FROM memory_entries m
                INNER JOIN thread t ON m.id = t.parent_id
            )
            SELECT * FROM thread ORDER BY timestamp ASC
            "#
        )
        .bind(response_id)
        .fetch_all(&self.pool)
        .await?;
        
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let id: i64 = row.get("id");
            let session_id: String = row.get("session_id");
            let role: String = row.get("role");
            let content: String = row.get("content");
            let timestamp: NaiveDateTime = row.get("timestamp");
            let tags: Option<String> = row.get("tags");
            
            let tags_vec = tags
                .as_ref()
                .and_then(|s| serde_json::from_str::<Vec<MemoryTag>>(s).ok());
            
            entries.push(MemoryEntry {
                id: Some(id),
                session_id,
                role,
                content,
                timestamp: Utc.from_utc_datetime(&timestamp),
                embedding: None,
                salience: None,
                tags: tags_vec,
                summary: None,
                memory_type: None,
                logprobs: None,
                moderation_flag: None,
                system_fingerprint: None,
                head: None,
                is_code: None,
                lang: None,
                topics: None,
                pinned: None,
                subject_tag: None,
                last_accessed: None,
            });
        }
        
        Ok(entries)
    }
    
    /// Save analysis data for a message
    pub async fn save_analysis(&self, message_id: i64, analysis: &MessageAnalysis) -> Result<()> {
        let topics_json = analysis.topics.as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or("[]".to_string()));
        let routed_json = analysis.routed_to_heads.as_ref()
            .map(|h| serde_json::to_string(h).unwrap_or("[]".to_string()));
        
        sqlx::query(
            r#"
            INSERT INTO message_analysis (
                message_id, mood, intensity, salience, intent, topics, summary,
                relationship_impact, contains_code, language, programming_lang,
                analysis_version, routed_to_heads
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(message_id) DO UPDATE SET
                mood = excluded.mood,
                intensity = excluded.intensity,
                salience = excluded.salience,
                intent = excluded.intent,
                topics = excluded.topics,
                summary = excluded.summary,
                analyzed_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(message_id)
        .bind(&analysis.mood)
        .bind(analysis.intensity)
        .bind(analysis.salience)
        .bind(&analysis.intent)
        .bind(topics_json)
        .bind(&analysis.summary)
        .bind(&analysis.relationship_impact)
        .bind(analysis.contains_code)
        .bind(&analysis.language)
        .bind(&analysis.programming_lang)
        .bind(&analysis.analysis_version)
        .bind(routed_json)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// Update recall metadata when a memory is recalled
    pub async fn update_recall_metadata(&self, message_id: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE message_analysis 
            SET last_recalled = CURRENT_TIMESTAMP,
                recall_count = COALESCE(recall_count, 0) + 1
            WHERE message_id = ?
            "#,
        )
        .bind(message_id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// Save a memory entry with explicit parent relationship
    pub async fn save_with_parent(&self, entry: &MemoryEntry, parent_id: Option<i64>) -> Result<MemoryEntry> {
        let tags_json = entry
            .tags
            .as_ref()
            .map(|tags| serde_json::to_string(tags).unwrap_or("[]".to_string()));

        // Generate response_id for assistant messages
        let response_id = match &entry.role[..] {
            "assistant" => Some(uuid::Uuid::new_v4().to_string()),
            _ => None,
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

        debug!("Saved memory entry {} with explicit parent {:?}", new_id, parent_id);
        Ok(saved_entry)
    }
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    async fn save(&self, entry: &MemoryEntry) -> Result<MemoryEntry> {
        let tags_json = entry
            .tags
            .as_ref()
            .map(|tags| serde_json::to_string(tags).unwrap_or("[]".to_string()));

        // Generate a response_id for tracking conversation threads
        let response_id = match &entry.role[..] {
            "assistant" => Some(uuid::Uuid::new_v4().to_string()),
            _ => None,
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

        debug!("Saved memory entry {} for session {} (parent: {:?}, response_id: {:?})", 
               new_id, entry.session_id, parent_id, response_id);
        Ok(saved_entry)
    }

    async fn load_recent(&self, session_id: &str, n: usize) -> Result<Vec<MemoryEntry>> {
        // Load with analysis data joined
        let rows = sqlx::query(
            r#"
            SELECT 
                m.id, m.session_id, m.role, m.content, m.timestamp, m.tags,
                a.salience, a.summary, a.topics, a.contains_code, a.programming_lang
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
        let mut ids: Vec<i64> = Vec::with_capacity(rows.len());

        for row in rows {
            let id: i64 = row.get("id");
            ids.push(id);

            let session_id: String = row.get("session_id");
            let role: String = row.get("role");
            let content: String = row.get("content");
            let timestamp: NaiveDateTime = row.get("timestamp");
            let tags: Option<String> = row.get("tags");
            let salience: Option<f32> = row.get("salience");
            let summary: Option<String> = row.get("summary");
            let topics: Option<String> = row.get("topics");
            let contains_code: Option<bool> = row.get("contains_code");
            let programming_lang: Option<String> = row.get("programming_lang");

            let tags_vec = tags
                .as_ref()
                .and_then(|s| serde_json::from_str::<Vec<MemoryTag>>(s).ok());
            
            let topics_vec = topics
                .as_ref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());

            entries.push(MemoryEntry {
                id: Some(id),
                session_id,
                role,
                content,
                timestamp: Utc.from_utc_datetime(&timestamp),
                embedding: None,  // In Qdrant
                salience,
                tags: tags_vec,
                summary,
                memory_type: None,
                logprobs: None,
                moderation_flag: None,
                system_fingerprint: None,
                head: None,
                is_code: contains_code,
                lang: programming_lang,
                topics: topics_vec,
                pinned: None,
                subject_tag: None,
                last_accessed: None,
            });
        }

        // Update recall metadata for all retrieved entries
        for id in ids {
            let _ = self.update_recall_metadata(id).await;
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
        debug!("Semantic search delegated to Qdrant");
        Ok(Vec::new())
    }

    async fn update_metadata(&self, id: i64, updated: &MemoryEntry) -> Result<MemoryEntry> {
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

    async fn delete(&self, id: i64) -> Result<()> {
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
}

// Analysis data structure matching message_analysis table
#[derive(Debug, Clone)]
pub struct MessageAnalysis {
    pub mood: Option<String>,
    pub intensity: Option<f32>,
    pub salience: Option<f32>,
    pub intent: Option<String>,
    pub topics: Option<Vec<String>>,
    pub summary: Option<String>,
    pub relationship_impact: Option<String>,
    pub contains_code: Option<bool>,
    pub language: Option<String>,
    pub programming_lang: Option<String>,
    pub analysis_version: Option<String>,
    pub routed_to_heads: Option<Vec<String>>,
}
