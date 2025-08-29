// src/llm/embeddings.rs
// Phase 3: Embeddings functionality using text-embedding-3-large
// PHASE 2: Added multi-head embedding components and implemented Display for EmbeddingHead

use crate::config::CONFIG;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt; // Import the fmt module
use std::str::FromStr;
use tokenizers::Tokenizer;

// --- Existing Code ---
// Embedding configuration for text-embedding-3-large
pub struct EmbeddingConfig {
    pub model: String,
    pub dimensions: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: "text-embedding-3-large".to_string(),
            dimensions: 3072,
        }
    }
}

// Response from the embeddings API
#[derive(Debug, Deserialize)]
pub struct EmbeddingResponse {
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: EmbeddingUsage,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingData {
    pub embedding: Vec<f32>,
    pub index: usize,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: usize,
    pub total_tokens: usize,
}

/// Helper functions for working with embeddings
pub mod utils {
    /// Calculate cosine similarity between two embeddings
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    /// Normalize an embedding vector
    pub fn normalize_embedding(embedding: &mut [f32]) {
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in embedding.iter_mut() {
                *value /= norm;
            }
        }
    }

    /// Calculate euclidean distance between two embeddings
    pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return f32::MAX;
        }

        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }
}

// --- PHASE 2: New Components for Multi-Head Embeddings ---

/// Represents the different embedding heads for multi-dimensional memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)] // Added Copy
pub enum EmbeddingHead {
    Semantic,
    Code,
    Summary,
}

impl EmbeddingHead {
    /// Returns the string representation of the embedding head.
    pub fn as_str(&self) -> &'static str {
        match self {
            EmbeddingHead::Semantic => "semantic",
            EmbeddingHead::Code => "code",
            EmbeddingHead::Summary => "summary",
        }
    }
}

// Corrected: Implement the Display trait for user-friendly printing
impl fmt::Display for EmbeddingHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Parses a string into an `EmbeddingHead`.
impl FromStr for EmbeddingHead {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "semantic" => Ok(EmbeddingHead::Semantic),
            "code" => Ok(EmbeddingHead::Code),
            "summary" => Ok(EmbeddingHead::Summary),
            _ => Err(anyhow::anyhow!("Unknown embedding head: {}", s)),
        }
    }
}

/// A utility for chunking text according to the strategy of a specific embedding head.
pub struct TextChunker {
    tokenizer: Tokenizer,
}

impl TextChunker {
    /// Creates a new `TextChunker` by loading a tokenizer model.
    pub fn new() -> Result<Self> {
        // This uses a pre-compiled tokenizer from Hugging Face (gpt2)
        // to avoid runtime file dependencies.
        let tokenizer = Tokenizer::from_bytes(include_bytes!("../../tokenizer.json"))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(Self { tokenizer })
    }

    /// Chunks the given text based on the rules for the specified embedding head.
    pub fn chunk_text(&self, text: &str, head: &EmbeddingHead) -> Result<Vec<String>> {
        let (chunk_size, chunk_overlap) = match head {
            EmbeddingHead::Semantic => (CONFIG.embed_semantic_chunk, CONFIG.embed_semantic_overlap),
            EmbeddingHead::Code => (CONFIG.embed_code_chunk, CONFIG.embed_code_overlap),
            EmbeddingHead::Summary => (CONFIG.embed_summary_chunk, CONFIG.embed_summary_overlap),
        };

        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let token_ids = encoding.get_ids();

        if token_ids.len() <= chunk_size {
            return Ok(vec![text.to_string()]);
        }

        let mut chunks = Vec::new();
        let mut start_idx = 0;
        let step = chunk_size.saturating_sub(chunk_overlap);

        while start_idx < token_ids.len() {
            let end_idx = std::cmp::min(start_idx + chunk_size, token_ids.len());
            let chunk_ids = &token_ids[start_idx..end_idx];

            let chunk_text = self
                .tokenizer
                .decode(chunk_ids, true)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            chunks.push(chunk_text);

            if step == 0 {
                break; // Avoid infinite loops if overlap is >= chunk size
            }
            if end_idx == token_ids.len() {
                break; // Reached the end
            }
            start_idx += step;
        }

        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::utils::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.0001);

        let c = vec![-1.0, -2.0, -3.0];
        assert!((cosine_similarity(&a, &c) + 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_normalize_embedding() {
        let mut embedding = vec![3.0, 4.0];
        normalize_embedding(&mut embedding);
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.0001);
    }
}

