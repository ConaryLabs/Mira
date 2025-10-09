// src/memory/storage/sqlite/core/analysis_operations.rs

use crate::memory::core::types::MemoryEntry;
use anyhow::Result;
use chrono::TimeZone;
use chrono::{NaiveDateTime, Utc};
use serde_json;
use sqlx::{Row, SqlitePool};
use tracing::debug;

/// Analysis data structure matching message_analysis table
#[derive(Debug, Clone)]
pub struct MessageAnalysis {
    pub mood: Option<String>,
    pub intensity: Option<f32>,
    pub salience: Option<f32>,
    pub original_salience: Option<f32>,
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

/// Handles message analysis storage and complex joins
pub struct AnalysisOperations {
    pool: SqlitePool,
}

impl AnalysisOperations {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store message analysis data
    pub async fn store_analysis(&self, message_id: i64, analysis: &MessageAnalysis) -> Result<()> {
        let topics_json = analysis.topics
            .as_ref()
            .map(|topics| serde_json::to_string(topics).unwrap_or("[]".to_string()));

        let routed_to_heads_json = analysis.routed_to_heads
            .as_ref()
            .map(|heads| serde_json::to_string(heads).unwrap_or("[]".to_string()));

        // Use salience as original_salience when first storing
        let original_salience = analysis.original_salience.or(analysis.salience);

        sqlx::query(
            r#"
            INSERT INTO message_analysis (
                message_id, mood, intensity, salience, original_salience, intent, topics, 
                summary, relationship_impact, contains_code, language, 
                programming_lang, analysis_version, routed_to_heads, analyzed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
            ON CONFLICT(message_id) DO UPDATE SET
                mood = excluded.mood,
                intensity = excluded.intensity,
                salience = excluded.salience,
                original_salience = COALESCE(message_analysis.original_salience, excluded.original_salience),
                intent = excluded.intent,
                topics = excluded.topics,
                summary = excluded.summary,
                relationship_impact = excluded.relationship_impact,
                contains_code = excluded.contains_code,
                language = excluded.language,
                programming_lang = excluded.programming_lang,
                analysis_version = excluded.analysis_version,
                routed_to_heads = excluded.routed_to_heads,
                analyzed_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(message_id)
        .bind(&analysis.mood)
        .bind(analysis.intensity)
        .bind(analysis.salience)
        .bind(original_salience)
        .bind(&analysis.intent)
        .bind(topics_json)
        .bind(&analysis.summary)
        .bind(&analysis.relationship_impact)
        .bind(analysis.contains_code.unwrap_or(false) as i64)
        .bind(&analysis.language)
        .bind(&analysis.programming_lang)
        .bind(&analysis.analysis_version)
        .bind(routed_to_heads_json)
        .execute(&self.pool)
        .await?;

        debug!("Stored analysis for message {} with original_salience={:?}", message_id, original_salience);
        Ok(())
    }

    /// Load memories with analysis data
    pub async fn load_memories_with_analysis(&self, session_id: &str, n: usize) -> Result<Vec<MemoryEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                m.id, m.session_id, m.role, m.content, m.timestamp, m.tags,
                m.response_id, m.parent_id,
                a.mood, a.intensity, a.salience, a.original_salience, a.intent, a.topics, a.summary,
                a.relationship_impact, a.contains_code, a.language, a.programming_lang,
                a.analysis_version, a.routed_to_heads, a.analyzed_at,
                a.last_recalled, a.recall_count
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

        let mut entries = Vec::new();

        for row in rows {
            let id: i64 = row.try_get("id")?;
            let session_id: String = row.try_get("session_id")?;
            let role: String = row.try_get("role")?;
            let content: String = row.try_get("content")?;
            let timestamp: i64 = row.try_get("timestamp")?;
            let tags: Option<String> = row.try_get("tags")?;
            let response_id: Option<String> = row.try_get("response_id")?;
            let parent_id: Option<i64> = row.try_get("parent_id")?;

            // Analysis fields
            let mood: Option<String> = row.try_get("mood")?;
            let intensity: Option<f32> = row.try_get("intensity")?;
            let salience: Option<f32> = row.try_get("salience")?;
            let original_salience: Option<f32> = row.try_get("original_salience")?;
            let intent: Option<String> = row.try_get("intent")?;
            let topics: Option<String> = row.try_get("topics")?;
            let summary: Option<String> = row.try_get("summary")?;
            let relationship_impact: Option<String> = row.try_get("relationship_impact")?;
            let contains_code: Option<i64> = row.try_get("contains_code")?;
            let language: Option<String> = row.try_get("language")?;
            let programming_lang: Option<String> = row.try_get("programming_lang")?;
            let analyzed_at: Option<NaiveDateTime> = row.try_get("analyzed_at")?;
            let analysis_version: Option<String> = row.try_get("analysis_version")?;
            let routed_to_heads: Option<String> = row.try_get("routed_to_heads")?;
            let last_recalled: Option<NaiveDateTime> = row.try_get("last_recalled")?;
            let recall_count: Option<i64> = row.try_get("recall_count")?;

            // Parse JSON fields
            let tags_vec: Option<Vec<String>> = tags
                .as_ref()
                .and_then(|t| serde_json::from_str(t).ok());

            let topics_vec: Option<Vec<String>> = topics
                .as_ref()
                .and_then(|t| serde_json::from_str(t).ok());

            let routed_to_heads_vec: Option<Vec<String>> = routed_to_heads
                .as_ref()
                .and_then(|h| serde_json::from_str(h).ok());

            let entry = MemoryEntry {
                id: Some(id),
                session_id,
                response_id,
                parent_id,
                role,
                content,
                timestamp: TimeZone::from_utc_datetime(&Utc, &NaiveDateTime::from_timestamp_opt(timestamp, 0).unwrap()),
                tags: tags_vec,
                mood,
                intensity,
                salience,
                original_salience,
                intent,
                topics: topics_vec,
                summary,
                relationship_impact,
                contains_code: contains_code.map(|c| c != 0),
                language,
                programming_lang,
                analyzed_at: analyzed_at.map(|dt| TimeZone::from_utc_datetime(&Utc, &dt)),
                analysis_version,
                routed_to_heads: routed_to_heads_vec,
                last_recalled: last_recalled.map(|dt| TimeZone::from_utc_datetime(&Utc, &dt)),
                recall_count,
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
            };

            entries.push(entry);
        }

        debug!("Loaded {} memories with analysis for session {}", entries.len(), session_id);
        Ok(entries)
    }

    /// Update recall metadata
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

        debug!("Updated recall metadata for message {}", message_id);
        Ok(())
    }

    /// Update only the salience (used by decay system)
    pub async fn update_salience(&self, message_id: i64, new_salience: f32) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE message_analysis 
            SET salience = ?
            WHERE message_id = ?
            "#,
        )
        .bind(new_salience)
        .bind(message_id)
        .execute(&self.pool)
        .await?;

        debug!("Updated salience for message {} to {}", message_id, new_salience);
        Ok(())
    }
}
