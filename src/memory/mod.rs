// src/memory/mod.rs

pub mod types;
pub mod traits;
pub mod recall;
pub mod salience;
pub mod summarizer;
pub mod decay;         // existing subject-aware decay logic (policies/helpers)
pub mod decay_scheduler; // NEW: background scheduler for periodic decay
pub mod sqlite;        // <--- THIS IS THE FIX
pub mod qdrant;
pub mod parallel_recall; // parallel optimization module

// Add the missing MemoryMessage type that ChatService needs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMessage {
    pub role: String,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// Re-export commonly used types and helpers
