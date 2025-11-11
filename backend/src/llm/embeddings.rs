// src/llm/embeddings.rs
// Embeddings functionality using text-embedding-3-large with multi-head support

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

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

pub mod utils {
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

    pub fn normalize_embedding(embedding: &mut [f32]) {
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in embedding.iter_mut() {
                *value /= norm;
            }
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmbeddingHead {
    Semantic,
    Code,
    Summary,
    Documents,
    Relationship,
}

impl EmbeddingHead {
    pub fn as_str(&self) -> &'static str {
        match self {
            EmbeddingHead::Semantic => "semantic",
            EmbeddingHead::Code => "code",
            EmbeddingHead::Summary => "summary",
            EmbeddingHead::Documents => "documents",
            EmbeddingHead::Relationship => "relationship",
        }
    }
    
    pub fn all() -> Vec<EmbeddingHead> {
        vec![
            EmbeddingHead::Semantic,
            EmbeddingHead::Code,
            EmbeddingHead::Summary,
            EmbeddingHead::Documents,
            EmbeddingHead::Relationship,
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
            "semantic" => Ok(EmbeddingHead::Semantic),
            "code" => Ok(EmbeddingHead::Code),
            "summary" => Ok(EmbeddingHead::Summary),
            "documents" => Ok(EmbeddingHead::Documents),
            "relationship" => Ok(EmbeddingHead::Relationship),
            _ => Err(anyhow::anyhow!("Unknown embedding head: {}", s)),
        }
    }
}
