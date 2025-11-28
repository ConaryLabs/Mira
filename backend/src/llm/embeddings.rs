// src/llm/embeddings.rs
// Embedding head types for multi-collection Qdrant storage

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Embedding collection types for Qdrant storage
///
/// Three collections (consolidated from previous 5):
/// - Code: Semantic nodes, code elements, design patterns, AST analysis
/// - Conversation: Messages, summaries, facts, user patterns, documents
/// - Git: Commits, co-change patterns, historical fixes, blame analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmbeddingHead {
    /// Code intelligence: semantic nodes, code elements, design patterns
    Code,
    /// Conversation data: messages, summaries, facts, user patterns, documents
    Conversation,
    /// Git intelligence: commits, co-change patterns, historical fixes
    Git,
}

impl EmbeddingHead {
    pub fn as_str(&self) -> &'static str {
        match self {
            EmbeddingHead::Code => "code",
            EmbeddingHead::Conversation => "conversation",
            EmbeddingHead::Git => "git",
        }
    }

    pub fn all() -> Vec<EmbeddingHead> {
        vec![
            EmbeddingHead::Code,
            EmbeddingHead::Conversation,
            EmbeddingHead::Git,
        ]
    }
}

impl fmt::Display for EmbeddingHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for EmbeddingHead {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "code" => Ok(EmbeddingHead::Code),
            "conversation" => Ok(EmbeddingHead::Conversation),
            "git" => Ok(EmbeddingHead::Git),
            _ => Err(anyhow::anyhow!("Unknown embedding head: {}", s)),
        }
    }
}
