// src/memory/types.rs

use crate::llm::classification::Classification;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr; // Import the FromStr trait

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: Option<i64>,                 // DB ID
    pub session_id: String,              // Session this belongs to
    pub role: String,                    // "user", "mira", etc.
    pub content: String,                 // Message content
    pub timestamp: DateTime<Utc>,        // When the message was created
    pub embedding: Option<Vec<f32>>,     // Embedding vector (if stored)
    pub salience: Option<f32>,           // Salience score (optional)
    pub tags: Option<Vec<MemoryTag>>,    // Tags for context/emotion
    pub summary: Option<String>,         // Short summary, if any
    pub memory_type: Option<MemoryType>, // Memory kind
    pub logprobs: Option<serde_json::Value>,
    pub moderation_flag: Option<bool>,
    pub system_fingerprint: Option<String>,

    // Fields for robust memory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_code: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<String>>,
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
            salience: Some(0.5), // Start with a default salience
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
        }
    }

    /// Updates the entry's metadata from a Classification object.
    pub fn with_classification(mut self, classification: Classification) -> Self {
        self.salience = Some(classification.salience);
        self.is_code = Some(classification.is_code);
        self.lang = Some(classification.lang);
        self.topics = Some(classification.topics);
        self
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

// Add this implementation block
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
