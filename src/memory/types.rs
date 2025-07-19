// src/memory/types.rs

use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Feeling,
    Fact,
    Joke,
    Promise,
    Event,
    #[serde(other)]
    Other,
}

pub type MemoryTag = String;
