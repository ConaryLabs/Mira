// src/llm/client/embedding.rs
// Handles text embedding generation using OpenAI's embedding models.
// SPRINT 2 OPTIMIZATION: Batch embedding support confirmed and optimized

use anyhow::{anyhow, Result};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info, warn};

use super::config::ClientConfig;

/// A client for generating text embeddings using the OpenAI API.
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

    /// Generates an embedding for a single text using the default model.
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.get_embedding_with_model(text, "text-embedding-3-large", Some(3072)).await
    }

    /// Generates an embedding for a single text with a specific model and dimensions.
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

        debug!("Requesting embedding for {} chars with model {}", text.len(), model);

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
            return Err(anyhow!("No embedding data in API response"));
        }

        let embedding = result.data[0].embedding.clone();
        
        info!("Generated embedding with {} dimensions", embedding.len());
        Ok(embedding)
    }

    /// SPRINT 2 OPTIMIZATION: Batch embedding implementation
    /// Generates embeddings for multiple texts in a single batch request using the default model.
    /// This reduces API calls by up to 100x for large batches!
    pub async fn get_embeddings_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.get_embeddings_batch_with_model(texts, "text-embedding-3-large", Some(3072)).await
    }

    /// SPRINT 2 OPTIMIZATION: Core batch embedding functionality
    /// Generates embeddings for multiple texts in a single batch request.
    /// 
    /// # Performance Notes
    /// - Processes up to 100 texts in a single API call
    /// - Reduces API calls by 90%+ compared to individual requests
    /// - Maintains exact order of embeddings matching input texts
    pub async fn get_embeddings_batch_with_model(
        &self,
        texts: &[String],
        model: &str,
        dimensions: Option<u32>,
    ) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        const MAX_BATCH_SIZE: usize = 100;
        if texts.len() > MAX_BATCH_SIZE {
            warn!("Batch size {} exceeds maximum of {}, consider splitting", texts.len(), MAX_BATCH_SIZE);
            return Err(anyhow!("Batch size {} exceeds maximum of {}", texts.len(), MAX_BATCH_SIZE));
        }

        let mut body = json!({
            "model": model,
            "input": texts,  // Send entire array at once!
        });

        if let Some(dims) = dimensions {
            body["dimensions"] = json!(dims);
        }

        info!("🚀 BATCH EMBEDDING: Requesting embeddings for {} texts in ONE API call", texts.len());
        debug!("Total characters to embed: {}", texts.iter().map(|t| t.len()).sum::<usize>());

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
            return Err(anyhow!("OpenAI batch embedding API error ({}): {}", status, error_text));
        }

        let result: EmbeddingResponse = response.json().await?;
        
        if result.data.len() != texts.len() {
            return Err(anyhow!(
                "Embedding count mismatch: expected {}, got {}",
                texts.len(),
                result.data.len()
            ));
        }

        // Sort by index to ensure correct order (OpenAI may return out of order)
        let mut sorted_data = result.data;
        sorted_data.sort_by_key(|item| item.index);
        
        let embeddings: Vec<Vec<f32>> = sorted_data
            .into_iter()
            .map(|item| item.embedding)
            .collect();
        
        info!("✅ BATCH EMBEDDING SUCCESS: Generated {} embeddings in 1 API call (saved {} calls!)", 
              texts.len(), texts.len() - 1);
        
        // Log token usage for cost tracking
        info!("📊 Token usage - Prompt: {}, Total: {}", 
              result.usage.prompt_tokens, 
              result.usage.total_tokens);
        
        Ok(embeddings)
    }

    /// Splits texts into optimal batches for processing
    /// Use this when you have more than 100 texts to embed
    pub async fn get_embeddings_batch_chunked(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        const MAX_BATCH_SIZE: usize = 100;
        let mut all_embeddings = Vec::new();
        
        for (batch_idx, chunk) in texts.chunks(MAX_BATCH_SIZE).enumerate() {
            info!("Processing batch {} of {}", batch_idx + 1, 
                  (texts.len() + MAX_BATCH_SIZE - 1) / MAX_BATCH_SIZE);
            
            let batch_embeddings = self.get_embeddings_batch(chunk).await?;
            all_embeddings.extend(batch_embeddings);
        }
        
        Ok(all_embeddings)
    }

    /// Calculates the cosine similarity between two embedding vectors.
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Result<f32> {
        if a.len() != b.len() {
            return Err(anyhow!("Embedding dimensions must match: {} vs {}", a.len(), b.len()));
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return Ok(0.0);
        }

        Ok(dot_product / (norm_a * norm_b))
    }
}

/// Utility functions related to embeddings.
pub struct EmbeddingUtils;

impl EmbeddingUtils {
    /// Splits long text into chunks suitable for embedding.
    pub fn chunk_text(text: &str, max_chunk_size: usize, overlap: usize) -> Vec<String> {
        if text.len() <= max_chunk_size {
            return vec![text.to_string()];
        }

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < text.len() {
            let end = std::cmp::min(start + max_chunk_size, text.len());
            chunks.push(text[start..end].to_string());
            if end >= text.len() {
                break;
            }
            start = end.saturating_sub(overlap);
        }
        chunks
    }

    /// Estimates the token count for embedding cost calculation
    /// Rough estimate: ~4 characters per token
    pub fn estimate_tokens(texts: &[String]) -> usize {
        texts.iter().map(|t| (t.len() + 3) / 4).sum()
    }

    /// Calculates estimated cost for embeddings
    /// Based on text-embedding-3-large pricing: $0.00013 per 1K tokens
    pub fn estimate_cost(texts: &[String]) -> f64 {
        let tokens = Self::estimate_tokens(texts);
        (tokens as f64 / 1000.0) * 0.00013
    }
}

// Internal structs for deserializing the OpenAI API response.
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

/// Contains information about a supported embedding model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModel {
    pub name: String,
    pub dimensions: u32,
    pub max_input_tokens: u32,
    pub description: String,
}

impl Default for EmbeddingModel {
    fn default() -> Self {
        Self {
            name: "text-embedding-3-large".to_string(),
            dimensions: 3072,
            max_input_tokens: 8191,
            description: "Latest and most capable embedding model".to_string(),
        }
    }
}
