// src/llm/client/embedding.rs
// Phase 4: Extract Embedding Operations from client.rs
// Handles text embedding generation using OpenAI's embedding models

use anyhow::{anyhow, Result};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info};

use super::config::ClientConfig;

/// Embedding client for text embeddings
pub struct EmbeddingClient {
    client: Client,
    config: ClientConfig,
}

impl EmbeddingClient {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    /// Get embedding for text using text-embedding-3-large with 3072 dimensions
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.get_embedding_with_model(text, "text-embedding-3-large", Some(3072)).await
    }

    /// Get embedding with specific model and dimensions
    pub async fn get_embedding_with_model(
        &self,
        text: &str,
        model: &str,
        dimensions: Option<u32>,
    ) -> Result<Vec<f32>> {
        let mut body = json!({
            "model": model,
            "input": text,
        });

        if let Some(dims) = dimensions {
            body["dimensions"] = json!(dims);
        }

        debug!("🔢 Requesting embedding for {} chars with model {}", text.len(), model);

        let response = self
            .client
            .post(format!("{}/v1/embeddings", self.config.base_url()))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow!("OpenAI embedding API error ({}): {}", status, error_text));
        }

        let result: EmbeddingResponse = response.json().await?;
        
        if result.data.is_empty() {
            return Err(anyhow!("No embedding data in response"));
        }

        let embedding = result.data[0].embedding.clone();
        
        info!("✅ Generated embedding: {} dimensions", embedding.len());
        Ok(embedding)
    }

    /// Get embeddings for multiple texts in batch
    pub async fn get_embeddings_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.get_embeddings_batch_with_model(texts, "text-embedding-3-large", Some(3072)).await
    }

    /// Get embeddings for multiple texts with specific model
    pub async fn get_embeddings_batch_with_model(
        &self,
        texts: &[String],
        model: &str,
        dimensions: Option<u32>,
    ) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Limit batch size to avoid API limits
        const MAX_BATCH_SIZE: usize = 100;
        if texts.len() > MAX_BATCH_SIZE {
            return Err(anyhow!("Batch size {} exceeds maximum {}", texts.len(), MAX_BATCH_SIZE));
        }

        let mut body = json!({
            "model": model,
            "input": texts,
        });

        if let Some(dims) = dimensions {
            body["dimensions"] = json!(dims);
        }

        debug!("🔢 Requesting embeddings for {} texts with model {}", texts.len(), model);

        let response = self
            .client
            .post(format!("{}/v1/embeddings", self.config.base_url()))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow!("OpenAI embedding API error ({}): {}", status, error_text));
        }

        let result: EmbeddingResponse = response.json().await?;
        
        if result.data.len() != texts.len() {
            return Err(anyhow!(
                "Embedding count mismatch: expected {}, got {}",
                texts.len(),
                result.data.len()
            ));
        }

        let embeddings = result.data.into_iter().map(|item| item.embedding).collect();
        
        info!("✅ Generated {} embeddings in batch", texts.len());
        Ok(embeddings)
    }

    /// Calculate cosine similarity between two embeddings
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Result<f32> {
        if a.len() != b.len() {
            return Err(anyhow!("Embedding dimensions don't match: {} vs {}", a.len(), b.len()));
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return Ok(0.0);
        }

        Ok(dot_product / (norm_a * norm_b))
    }

    /// Calculate Euclidean distance between two embeddings
    pub fn euclidean_distance(a: &[f32], b: &[f32]) -> Result<f32> {
        if a.len() != b.len() {
            return Err(anyhow!("Embedding dimensions don't match: {} vs {}", a.len(), b.len()));
        }

        let sum_squared_diff: f32 = a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum();

        Ok(sum_squared_diff.sqrt())
    }

    /// Normalize embedding vector to unit length
    pub fn normalize_embedding(embedding: &mut [f32]) {
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in embedding.iter_mut() {
                *value /= norm;
            }
        }
    }

    /// Get supported embedding models
    pub fn supported_models() -> Vec<EmbeddingModel> {
        vec![
            EmbeddingModel {
                name: "text-embedding-3-large".to_string(),
                dimensions: 3072,
                max_input_tokens: 8192,
                description: "Most capable embedding model".to_string(),
            },
            EmbeddingModel {
                name: "text-embedding-3-small".to_string(),
                dimensions: 1536,
                max_input_tokens: 8192,
                description: "Smaller, faster embedding model".to_string(),
            },
            EmbeddingModel {
                name: "text-embedding-ada-002".to_string(),
                dimensions: 1536,
                max_input_tokens: 8192,
                description: "Legacy embedding model".to_string(),
            },
        ]
    }

    /// Validate text length for embedding
    pub fn validate_text_length(text: &str, model: &str) -> Result<()> {
        // Rough token estimation (1 token ≈ 4 characters)
        let estimated_tokens = text.len() / 4;
        let max_tokens = 8192; // Most embedding models support 8k tokens

        if estimated_tokens > max_tokens {
            return Err(anyhow!(
                "Text too long for model {}: ~{} tokens (max: {})",
                model,
                estimated_tokens,
                max_tokens
            ));
        }

        Ok(())
    }
}

/// Embedding response from OpenAI API
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
    model: String,
    usage: EmbeddingUsage,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Debug, Deserialize)]
struct EmbeddingUsage {
    prompt_tokens: u32,
    total_tokens: u32,
}

/// Information about an embedding model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModel {
    pub name: String,
    pub dimensions: u32,
    pub max_input_tokens: u32,
    pub description: String,
}

/// Embedding utility functions
pub struct EmbeddingUtils;

impl EmbeddingUtils {
    /// Split long text into chunks suitable for embedding
    pub fn chunk_text(text: &str, max_chunk_size: usize, overlap: usize) -> Vec<String> {
        if text.len() <= max_chunk_size {
            return vec![text.to_string()];
        }

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < text.len() {
            let end = std::cmp::min(start + max_chunk_size, text.len());
            let chunk = text[start..end].to_string();
            chunks.push(chunk);

            if end >= text.len() {
                break;
            }

            start = end.saturating_sub(overlap);
        }

        chunks
    }

    /// Find most similar embeddings using cosine similarity
    pub fn find_most_similar(
        query_embedding: &[f32],
        embeddings: &[(Vec<f32>, String)], // (embedding, text)
        top_k: usize,
    ) -> Result<Vec<(f32, String)>> {
        if embeddings.is_empty() {
            return Ok(Vec::new());
        }

        let mut similarities: Vec<(f32, String)> = embeddings
            .iter()
            .map(|(emb, text)| {
                let similarity = EmbeddingClient::cosine_similarity(query_embedding, emb)
                    .unwrap_or(0.0);
                (similarity, text.clone())
            })
            .collect();

        // Sort by similarity (descending)
        similarities.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Take top k
        similarities.truncate(top_k);

        Ok(similarities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let similarity = EmbeddingClient::cosine_similarity(&a, &b).unwrap();
        assert!((similarity - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        let similarity = EmbeddingClient::cosine_similarity(&a, &c).unwrap();
        assert!((similarity - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_euclidean_distance() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![3.0, 4.0, 0.0];
        let distance = EmbeddingClient::euclidean_distance(&a, &b).unwrap();
        assert!((distance - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_embedding() {
        let mut embedding = vec![3.0, 4.0, 0.0];
        EmbeddingClient::normalize_embedding(&mut embedding);
        
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_chunk_text() {
        let text = "This is a long text that needs to be chunked for embedding processing.";
        let chunks = EmbeddingUtils::chunk_text(text, 20, 5);
        
        assert!(chunks.len() > 1);
        assert!(chunks[0].len() <= 20);
    }

    #[test]
    fn test_validate_text_length() {
        let short_text = "Short text";
        assert!(EmbeddingClient::validate_text_length(short_text, "text-embedding-3-large").is_ok());

        let long_text = "a".repeat(50000); // Very long text
        assert!(EmbeddingClient::validate_text_length(&long_text, "text-embedding-3-large").is_err());
    }

    #[test]
    fn test_find_most_similar() {
        let query = vec![1.0, 0.0, 0.0];
        let embeddings = vec![
            (vec![1.0, 0.0, 0.0], "exact match".to_string()),
            (vec![0.8, 0.6, 0.0], "close match".to_string()),
            (vec![0.0, 1.0, 0.0], "no match".to_string()),
        ];

        let results = EmbeddingUtils::find_most_similar(&query, &embeddings, 2).unwrap();
        
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1, "exact match");
        assert!(results[0].0 > results[1].0); // First should have higher similarity
    }

    #[test]
    fn test_supported_models() {
        let models = EmbeddingClient::supported_models();
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.name == "text-embedding-3-large"));
    }
}
