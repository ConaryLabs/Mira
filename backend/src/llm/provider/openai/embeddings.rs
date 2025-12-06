// backend/src/llm/provider/openai/embeddings.rs
// OpenAI Embeddings provider using text-embedding-3-large

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// OpenAI Embedding models
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAIEmbeddingModel {
    /// text-embedding-3-large: 3072 dimensions, best quality
    TextEmbedding3Large,
    /// text-embedding-3-small: 1536 dimensions, faster/cheaper
    TextEmbedding3Small,
}

impl OpenAIEmbeddingModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            OpenAIEmbeddingModel::TextEmbedding3Large => "text-embedding-3-large",
            OpenAIEmbeddingModel::TextEmbedding3Small => "text-embedding-3-small",
        }
    }

    pub fn dimensions(&self) -> usize {
        match self {
            OpenAIEmbeddingModel::TextEmbedding3Large => 3072,
            OpenAIEmbeddingModel::TextEmbedding3Small => 1536,
        }
    }
}

/// OpenAI Embeddings provider
/// Uses text-embedding-3-large model (3072 dimensions, same as Gemini gemini-embedding-001)
pub struct OpenAIEmbeddings {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

#[derive(Serialize)]
struct EmbeddingRequest {
    input: EmbeddingInput,
    model: String,
}

#[derive(Serialize)]
#[serde(untagged)]
enum EmbeddingInput {
    Single(String),
    Batch(Vec<String>),
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
    #[allow(dead_code)]
    model: String,
    usage: EmbeddingUsage,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Deserialize)]
struct EmbeddingUsage {
    #[allow(dead_code)]
    prompt_tokens: u32,
    total_tokens: u32,
}

impl OpenAIEmbeddings {
    /// Create a new OpenAI embeddings provider with text-embedding-3-large
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, OpenAIEmbeddingModel::TextEmbedding3Large)
    }

    /// Create with specific embedding model
    pub fn with_model(api_key: String, model: OpenAIEmbeddingModel) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model: model.as_str().to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }

    /// Create with custom model string
    pub fn with_model_name(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }

    /// Build the API URL for embeddings
    fn api_url(&self) -> String {
        format!("{}/embeddings", self.base_url)
    }

    /// Generate embedding for a single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        debug!(
            "Generating OpenAI embedding for text ({} chars)",
            text.len()
        );

        let request = EmbeddingRequest {
            input: EmbeddingInput::Single(text.to_string()),
            model: self.model.clone(),
        };

        let response = self
            .client
            .post(self.api_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("OpenAI API error {}: {}", status, error_text));
        }

        let result: EmbeddingResponse = response.json().await?;

        let embedding = result
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No embedding in OpenAI response"))?
            .embedding;

        debug!(
            "Generated embedding with {} dimensions, {} tokens",
            embedding.len(),
            result.usage.total_tokens
        );
        Ok(embedding)
    }

    /// Generate embeddings for multiple texts in a single API call (batch optimization)
    pub async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        info!("Generating OpenAI embeddings for {} texts", texts.len());

        let request = EmbeddingRequest {
            input: EmbeddingInput::Batch(texts.clone()),
            model: self.model.clone(),
        };

        let response = self
            .client
            .post(self.api_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("OpenAI API error {}: {}", status, error_text));
        }

        let result: EmbeddingResponse = response.json().await?;

        // Sort by index to maintain order
        let mut embeddings: Vec<(usize, Vec<f32>)> = result
            .data
            .into_iter()
            .map(|d| (d.index, d.embedding))
            .collect();
        embeddings.sort_by_key(|(idx, _)| *idx);

        let embeddings: Vec<Vec<f32>> = embeddings.into_iter().map(|(_, e)| e).collect();

        info!(
            "Generated {} embeddings with {} dimensions each, {} total tokens",
            embeddings.len(),
            embeddings.first().map(|e| e.len()).unwrap_or(0),
            result.usage.total_tokens
        );

        Ok(embeddings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_properties() {
        assert_eq!(
            OpenAIEmbeddingModel::TextEmbedding3Large.as_str(),
            "text-embedding-3-large"
        );
        assert_eq!(OpenAIEmbeddingModel::TextEmbedding3Large.dimensions(), 3072);

        assert_eq!(
            OpenAIEmbeddingModel::TextEmbedding3Small.as_str(),
            "text-embedding-3-small"
        );
        assert_eq!(OpenAIEmbeddingModel::TextEmbedding3Small.dimensions(), 1536);
    }

    #[test]
    fn test_api_url_construction() {
        let provider = OpenAIEmbeddings::new("test_key".to_string());
        let url = provider.api_url();
        assert_eq!(url, "https://api.openai.com/v1/embeddings");
    }

    #[test]
    fn test_default_model() {
        let provider = OpenAIEmbeddings::new("test_key".to_string());
        assert_eq!(provider.model, "text-embedding-3-large");
    }
}
