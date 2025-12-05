// src/memory/core/types.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    // Core fields from memory_entries table
    pub id: Option<i64>,
    pub session_id: String,
    pub response_id: Option<String>,
    pub parent_id: Option<i64>,
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub tags: Option<Vec<String>>,

    // Fields from message_analysis table
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
    pub analyzed_at: Option<DateTime<Utc>>,
    pub analysis_version: Option<String>,
    pub routed_to_heads: Option<Vec<String>>,
    pub last_recalled: Option<DateTime<Utc>>,
    pub recall_count: Option<i64>,

    // Error tracking fields
    pub contains_error: Option<bool>,
    pub error_type: Option<String>,
    pub error_severity: Option<String>,
    pub error_file: Option<String>,

    // Fields from llm_metadata table
    pub model_version: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    pub generation_time_ms: Option<i64>,
    pub finish_reason: Option<String>,
    pub tool_calls: Option<Vec<String>>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i64>,

    // Embedding info
    pub embedding: Option<Vec<f32>>,
    pub embedding_heads: Option<Vec<String>>,
    pub qdrant_point_ids: Option<Vec<String>>,
}

impl MemoryEntry {
    pub fn user_message(session_id: String, content: String) -> Self {
        Self {
            id: None,
            session_id,
            response_id: None,
            parent_id: None,
            role: "user".to_string(),
            content,
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
            contains_code: None,
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
        }
    }

    pub fn assistant_message(session_id: String, content: String) -> Self {
        let mut entry = Self::user_message(session_id, content);
        entry.role = "assistant".to_string();
        entry
    }

    pub fn document(session_id: String, content: String, file_path: &str) -> Self {
        let mut entry = Self::user_message(session_id, content);
        entry.role = "document".to_string();
        entry.tags = Some(vec!["document".to_string(), format!("file:{}", file_path)]);
        entry
    }

    /// Check if this entry has high salience for memory storage
    pub fn is_high_salience(&self, threshold: f32) -> bool {
        self.salience.map_or(false, |s| s >= threshold)
    }

    /// Get the age of this memory entry
    pub fn age_hours(&self) -> i64 {
        (Utc::now() - self.timestamp).num_hours()
    }

    /// Check if this entry contains code
    pub fn has_code(&self) -> bool {
        self.contains_code.unwrap_or(false) || self.programming_lang.is_some()
    }

    /// Check if this entry contains an error
    pub fn has_error(&self) -> bool {
        self.contains_error.unwrap_or(false)
    }

    /// Check if this is a high-severity error
    pub fn is_critical_error(&self) -> bool {
        self.contains_error.unwrap_or(false)
            && self
                .error_severity
                .as_ref()
                .map(|s| matches!(s.to_lowercase().as_str(), "critical" | "high"))
                .unwrap_or(false)
    }

    /// Extract project ID from tags (stored as "project:xyz")
    pub fn extract_project_from_tags(&self) -> Option<String> {
        self.tags.as_ref().and_then(|tags| {
            tags.iter()
                .find(|t| t.starts_with("project:"))
                .and_then(|t| t.strip_prefix("project:"))
                .map(|s| s.to_string())
        })
    }
}
