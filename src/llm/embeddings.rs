// src/llm/embeddings.rs
// Embeddings functionality using text-embedding-3-large with multi-head support including Documents

use crate::config::CONFIG;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use tokenizers::Tokenizer;

/// Embedding configuration for text-embedding-3-large
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

/// Response from the embeddings API
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

/// Represents the different embedding heads for multi-dimensional memory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmbeddingHead {
    Semantic,   // General conversation embeddings
    Code,       // Programming-specific embeddings
    Summary,    // Rolling summary embeddings
    Documents,  // Project documents (PDFs, markdown, etc.)
}

impl EmbeddingHead {
    /// Returns the string representation of the embedding head
    pub fn as_str(&self) -> &'static str {
        match self {
            EmbeddingHead::Semantic => "semantic",
            EmbeddingHead::Code => "code",
            EmbeddingHead::Summary => "summary",
            EmbeddingHead::Documents => "documents",
        }
    }
    
    /// Get all available heads
    pub fn all() -> Vec<EmbeddingHead> {
        vec![
            EmbeddingHead::Semantic,
            EmbeddingHead::Code,
            EmbeddingHead::Summary,
            EmbeddingHead::Documents,
        ]
    }
    
    /// Check if this head should be used for a given content type
    pub fn should_route(&self, content: &str, role: &str) -> bool {
        match self {
            EmbeddingHead::Semantic => {
                // Always route normal conversation here
                role == "user" || role == "assistant"
            },
            EmbeddingHead::Code => {
                // Route if content contains code indicators
                contains_code_indicators(content)
            },
            EmbeddingHead::Summary => {
                // Only for system-generated summaries
                role == "system" && content.contains("Summary:")
            },
            EmbeddingHead::Documents => {
                // For uploaded documents
                role == "document"
            },
        }
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
            "semantic" => Ok(EmbeddingHead::Semantic),
            "code" => Ok(EmbeddingHead::Code),
            "summary" => Ok(EmbeddingHead::Summary),
            "documents" => Ok(EmbeddingHead::Documents),
            _ => Err(anyhow::anyhow!("Unknown embedding head: {}", s)),
        }
    }
}

/// A utility for chunking text according to the strategy of a specific embedding head
pub struct TextChunker {
    tokenizer: Tokenizer,
}

impl TextChunker {
    /// Creates a new TextChunker by loading a tokenizer model
    pub fn new() -> Result<Self> {
        let tokenizer = Tokenizer::from_bytes(include_bytes!("../../tokenizer.json"))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(Self { tokenizer })
    }

    /// Chunks the given text based on the rules for the specified embedding head
    pub fn chunk_text(&self, text: &str, head: &EmbeddingHead) -> Result<Vec<String>> {
        let (chunk_size, chunk_overlap) = match head {
            EmbeddingHead::Semantic => (CONFIG.embed_semantic_chunk, CONFIG.embed_semantic_overlap),
            EmbeddingHead::Code => (CONFIG.embed_code_chunk, CONFIG.embed_code_overlap),
            EmbeddingHead::Summary => (CONFIG.embed_summary_chunk, CONFIG.embed_summary_overlap),
            EmbeddingHead::Documents => (
                CONFIG.embed_document_chunk,  // No longer Optional
                CONFIG.embed_document_overlap  // No longer Optional
            ),
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
                break;
            }
            if end_idx == token_ids.len() {
                break;
            }
            start_idx += step;
        }

        Ok(chunks)
    }
}

/// Helper function to check if content contains code indicators
fn contains_code_indicators(content: &str) -> bool {
    // Common code patterns
    let code_patterns = [
        "```",           // Markdown code blocks
        "fn ",           // Rust functions
        "def ",          // Python functions
        "function ",     // JavaScript
        "class ",        // Classes
        "import ",       // Imports
        "const ",        // Constants
        "let ",          // Variables
        "var ",          // Variables
        "return ",       // Returns
        "if (",          // Conditionals
        "for (",         // Loops
        "while (",       // Loops
        "SELECT ",       // SQL
        "CREATE TABLE",  // SQL
        "pub struct",    // Rust
        "async fn",      // Rust async
        "#include",      // C/C++
    ];
    
    let lower_content = content.to_lowercase();
    code_patterns.iter().any(|pattern| lower_content.contains(&pattern.to_lowercase()))
}
