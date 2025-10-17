// src/memory/service/core_service.rs
use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use crate::memory::{
    storage::sqlite::store::SqliteMemoryStore,
    storage::qdrant::multi_store::QdrantMultiStore,
    core::types::MemoryEntry,
    core::traits::MemoryStore,
};

pub struct MemoryCoreService {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub multi_store: Arc<QdrantMultiStore>,
}

impl MemoryCoreService {
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            sqlite_store,
            multi_store,
        }
    }

    /// Save a user message and return the entry ID
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<i64> {
        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            response_id: None,
            parent_id: None,
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            tags: project_id.map(|pid| vec![format!("project:{}", pid)]),
            mood: None,
            intensity: None,
            salience: None,
            original_salience: None,
            intent: None,
            topics: None,
            summary: None,
            relationship_impact: None,
            contains_code: Some(false),
            language: Some("en".to_string()),
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
            embedding: None,
            embedding_heads: None,
            qdrant_point_ids: None,
        };

        let saved = self.sqlite_store.save(&entry).await?;
        Ok(saved.id.unwrap_or(0))
    }

    /// Save an assistant message and return the entry ID
    pub async fn save_assistant_message(
        &self,
        session_id: &str,
        content: &str,
        parent_id: Option<i64>,
    ) -> Result<i64> {
        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            response_id: Some(uuid::Uuid::new_v4().to_string()),
            parent_id,
            role: "assistant".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            tags: None,
            mood: None,
            intensity: None,
            salience: None,
            original_salience: None,
            intent: None,
            topics: None,
            summary: None,
            relationship_impact: None,
            contains_code: Some(false),
            language: Some("en".to_string()),
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
            embedding: None,
            embedding_heads: None,
            qdrant_point_ids: None,
        };

        let saved = self.sqlite_store.save(&entry).await?;
        Ok(saved.id.unwrap_or(0))
    }

    /// Save a memory entry and return the entry ID
    pub async fn save_entry(&self, entry: &MemoryEntry) -> Result<i64> {
        let saved_entry = self.sqlite_store.save(entry).await?;
        Ok(saved_entry.id.unwrap_or(0))
    }

    /// Get recent memories for a session
    pub async fn get_recent(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.sqlite_store.load_recent(session_id, limit).await
    }

    /// Get service statistics
    pub async fn get_stats(&self, session_id: &str) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "session_id": session_id,
            "status": "operational"
        }))
    }

    /// Cleanup inactive sessions
    pub async fn cleanup_inactive_sessions(&self, _max_age_hours: i64) -> Result<usize> {
        // Cleanup logic will be implemented here
        // For now, return 0
        Ok(0)
    }
}
