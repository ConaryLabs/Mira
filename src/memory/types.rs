// src/memory/types.rs

use crate::llm::classification::Classification;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Primary record for persisted conversational/memory items.
/// Phase 4 adds pinning, subject-aware tagging, and last-access tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    // Core identity & content
    pub id: Option<i64>,                 // DB ID
    pub session_id: String,              // Session this belongs to
    pub role: String,                    // "user", "mira", etc.
    pub content: String,                 // Message content
    pub timestamp: DateTime<Utc>,        // When the message was created

    // Vector / ranking
    pub embedding: Option<Vec<f32>>,     // Embedding vector (if stored)
    pub salience: Option<f32>,           // Salience score (optional)

    // Metadata
    pub tags: Option<Vec<MemoryTag>>,    // Tags for context/emotion
    pub summary: Option<String>,         // Short summary, if any
    pub memory_type: Option<MemoryType>, // Memory kind
    pub logprobs: Option<serde_json::Value>,
    pub moderation_flag: Option<bool>,
    pub system_fingerprint: Option<String>,

    // Robust memory (Phase 3)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_code: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<String>>,

    // Phase 4: Pinning, subject-aware decay, and recency
    /// If true, entry is immune to decay and prioritized for recall.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,

    /// Subject/semantic tag for decay weighting, e.g. "birthday", "anniversary", "project:mira", "general".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_tag: Option<String>,

    /// Tracks last read/write access; decay uses this instead of `timestamp` when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accessed: Option<DateTime<Utc>>,
}

impl MemoryEntry {
    /// Creates a new, basic MemoryEntry from a user message.
    pub fn from_user_message(session_id: String, content: String) -> Self {
        Self {
            id: None,
            session_id,
            role: "user".to_string(),
            content,
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(0.5), // Start with a reasonable default salience
            tags: Some(Vec::new()),
            summary: None,
            memory_type: Some(MemoryType::Event),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: None,
            lang: None,
            topics: None,

            // Phase 4 defaults
            pinned: Some(false),
            subject_tag: None,
            last_accessed: Some(Utc::now()),
        }
    }

    /// Apply LLM-derived classification to enrich metadata.
    pub fn with_classification(mut self, classification: Classification) -> Self {
        self.salience = Some(classification.salience);
        self.is_code = Some(classification.is_code);
        self.lang = Some(classification.lang);
        self.topics = Some(classification.topics);
        self
    }

    /// Mark the entry as accessed "now" and optionally apply a small salience boost (clamped).
    /// Call this when you surface the memory in context or update it.
    pub fn touch(&mut self, boost: Option<f32>) {
        self.last_accessed = Some(Utc::now());
        if let Some(b) = boost {
            let current = self.salience.unwrap_or(0.5);
            // clamp boost 0.0..=0.5 and salience to 0.0..=1.0
            let b = b.clamp(0.0, 0.5);
            self.salience = Some((current + b).clamp(0.0, 1.0));
        }
    }

    /// Convenience setters to align with Phase 4 features.
    pub fn set_subject_tag<S: Into<String>>(&mut self, tag: S) {
        self.subject_tag = Some(tag.into());
    }

    pub fn pin(&mut self) {
        self.pinned = Some(true);
    }

    pub fn unpin(&mut self) {
        self.pinned = Some(false);
    }
}

// FIX: Manually implement the Default trait for MemoryEntry
impl Default for MemoryEntry {
    fn default() -> Self {
        Self {
            id: None,
            session_id: String::new(),
            role: String::new(),
            content: String::new(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(0.0),
            tags: Some(Vec::new()),
            summary: None,
            memory_type: Some(MemoryType::Other),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
            head: None,
            is_code: None,
            lang: None,
            topics: None,
            pinned: Some(false),
            subject_tag: None,
            last_accessed: Some(Utc::now()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Feeling,
    Fact,
    Joke,
    Promise,
    Event,
    Summary,
    #[serde(other)]
    Other,
}

// Parse MemoryType from strings defensively (DB/text interop)
impl FromStr for MemoryType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "feeling" => MemoryType::Feeling,
            "fact" => MemoryType::Fact,
            "joke" => MemoryType::Joke,
            "promise" => MemoryType::Promise,
            "event" => MemoryType::Event,
            "summary" => MemoryType::Summary,
            _ => MemoryType::Other,
        })
    }
}

pub type MemoryTag = String;
