// src/llm/client/embedding.rs
// Handles text embedding generation using OpenAI's embedding models.

use anyhow::{anyhow, Result};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info};

use crate::config::CONFIG;

/// A client for generating text embeddings using the OpenAI API.
pub struct EmbeddingClient {
    client: Client,
}

impl EmbeddingClient {
    pub fn new(_config: super::config::ClientConfig) -> Self {
        // Note: config parameter is unused - we always hardcode to OpenAI's API
        // Kept for API compatibility with existing callers
        Self {
            client: Client::new(),
        }
    }

    /// Generates an embedding for a single text using the default model.
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.get_embedding_with_model(text, &CONFIG.openai_embedding_model, Some(3072)).await
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

        // Always use OpenAI's API for embeddings
        let response = self
            .client
            .post("https://api.openai.com/v1/embeddings")
            .header(header::AUTHORIZATION, format!("Bearer {}", &CONFIG.openai_embedding_api_key))
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

    /// Generates embeddings for multiple texts in a single batch request.
    /// Processes up to 100 texts in a single API call, reducing API calls by 90%+.
    pub async fn get_embeddings_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.get_embeddings_batch_with_model(texts, &CONFIG.openai_embedding_model, Some(3072)).await
    }

    /// Generates embeddings for multiple texts with a specific model.
    /// Maintains exact order of embeddings matching input texts.
    pub async fn get_embeddings_batch_with_model(
        &self,
        texts: &[String],
        model: &str,
        dimensions: Option<u32>,
    ) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut body = json!({
            "model": model,
            "input": texts,
        });

        if let Some(dims) = dimensions {
            body["dimensions"] = json!(dims);
        }

        debug!(
            "Requesting batch embeddings for {} texts (total {} chars) with model {}",
            texts.len(),
            texts.iter().map(|t| t.len()).sum::<usize>(),
            model
        );

        // Always use OpenAI's API for embeddings
        let response = self
            .client
            .post("https://api.openai.com/v1/embeddings")
            .header(header::AUTHORIZATION, format!("Bearer {}", &CONFIG.openai_embedding_api_key))
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

        // Sort by index to maintain original order
        let mut sorted_data = result.data;
        sorted_data.sort_by_key(|d| d.index);

        let embeddings: Vec<Vec<f32>> = sorted_data
            .into_iter()
            .map(|d| d.embedding)
            .collect();

        info!(
            "Generated {} embeddings with {} dimensions each",
            embeddings.len(),
            embeddings.first().map(|e| e.len()).unwrap_or(0)
        );

        Ok(embeddings)
    }

    /// Calculates the number of batches needed for texts (OpenAI limit: 100 texts per batch).
    pub fn calculate_batches(texts: &[String]) -> Vec<Vec<String>> {
        const BATCH_SIZE: usize = 100;
        texts
            .chunks(BATCH_SIZE)
            .map(|chunk| chunk.to_vec())
            .collect()
    }

    /// Estimates the number of tokens in a batch of texts.
    /// Rough estimate: ~4 characters per token.
    pub fn estimate_tokens(texts: &[String]) -> usize {
        texts.iter().map(|t| (t.len() + 3) / 4).sum()
    }

    /// Calculates estimated cost for embeddings.
    /// Based on text-embedding-3-large pricing: $0.00013 per 1K tokens.
    pub fn estimate_cost(texts: &[String]) -> f64 {
        let tokens = Self::estimate_tokens(texts);
        (tokens as f64 / 1000.0) * 0.00013
    }
}

// Internal structs for deserializing the OpenAI API response.
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
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
