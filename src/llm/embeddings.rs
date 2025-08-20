// src/llm/embeddings.rs
// Phase 3: Embeddings functionality using text-embedding-3-large

use serde::Deserialize;

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
