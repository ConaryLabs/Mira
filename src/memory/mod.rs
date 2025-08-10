// src/memory/mod.rs

pub mod types;
pub mod traits;
pub mod recall;
pub mod salience;
pub mod summarizer;
pub mod decay;
pub mod sqlite;
pub mod qdrant;

// Add the missing MemoryMessage type that ChatService needs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMessage {
    pub role: String,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// Re-export commonly used types from submodules if they exist
pub use types::*;
