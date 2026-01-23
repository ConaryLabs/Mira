// crates/mira-server/src/embeddings.rs
// OpenAI embeddings API client

use crate::db::{Database, EmbeddingUsageRecord};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::debug;

/// Max characters to embed (truncate longer text)
const MAX_TEXT_CHARS: usize = 8000;

/// Max batch size for batch embedding (OpenAI supports up to 2048)
const MAX_BATCH_SIZE: usize = 100;

/// HTTP timeout
const TIMEOUT_SECS: u64 = 30;

/// Retry attempts
const RETRY_ATTEMPTS: usize = 2;

/// API endpoint
const API_URL: &str = "https://api.openai.com/v1/embeddings";

/// Supported embedding models
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddingModel {
    /// text-embedding-3-small: 1536 dimensions, $0.02/1M tokens (recommended)
    #[default]
    TextEmbedding3Small,
    /// text-embedding-3-large: 3072 dimensions, $0.13/1M tokens
    TextEmbedding3Large,
    /// text-embedding-ada-002: 1536 dimensions, $0.10/1M tokens (legacy)
    TextEmbeddingAda002,
}

impl EmbeddingModel {
    /// Get the model name for API calls
    pub fn model_name(&self) -> &'static str {
        match self {
            Self::TextEmbedding3Small => "text-embedding-3-small",
            Self::TextEmbedding3Large => "text-embedding-3-large",
            Self::TextEmbeddingAda002 => "text-embedding-ada-002",
        }
    }

    /// Get embedding dimensions for this model
    pub fn dimensions(&self) -> usize {
        match self {
            Self::TextEmbedding3Small => 1536,
            Self::TextEmbedding3Large => 3072,
            Self::TextEmbeddingAda002 => 1536,
        }
    }

    /// Get cost per million tokens
    pub fn cost_per_million(&self) -> f64 {
        match self {
            Self::TextEmbedding3Small => 0.02,
            Self::TextEmbedding3Large => 0.13,
            Self::TextEmbeddingAda002 => 0.10,
        }
    }

    /// Parse from model name string
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "text-embedding-3-small" => Some(Self::TextEmbedding3Small),
            "text-embedding-3-large" => Some(Self::TextEmbedding3Large),
            "text-embedding-ada-002" => Some(Self::TextEmbeddingAda002),
            _ => None,
        }
    }

    /// List all available models
    pub fn all() -> &'static [EmbeddingModel] {
        &[
            Self::TextEmbedding3Small,
            Self::TextEmbedding3Large,
            Self::TextEmbeddingAda002,
        ]
    }
}

impl std::fmt::Display for EmbeddingModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.model_name())
    }
}

/// Default embedding dimensions (for backwards compatibility with EMBEDDING_DIM constant)
pub const EMBEDDING_DIM: usize = 1536;

/// Embeddings client
pub struct Embeddings {
    api_key: String,
    model: EmbeddingModel,
    http_client: reqwest::Client,
    db: Option<Arc<Database>>,
    project_id: Arc<RwLock<Option<i64>>>,
}

impl Embeddings {
    /// Create new embeddings client with default model (text-embedding-3-small)
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, EmbeddingModel::default(), None)
    }

    /// Create embeddings client with database for usage tracking
    pub fn with_db(api_key: String, db: Option<Arc<Database>>) -> Self {
        Self::with_model(api_key, EmbeddingModel::default(), db)
    }

    /// Create embeddings client with specific model and optional database
    pub fn with_model(api_key: String, model: EmbeddingModel, db: Option<Arc<Database>>) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self::with_http_client(api_key, model, db, http_client)
    }

    /// Create embeddings client with a shared HTTP client
    pub fn with_http_client(
        api_key: String,
        model: EmbeddingModel,
        db: Option<Arc<Database>>,
        http_client: reqwest::Client,
    ) -> Self {
        Self {
            api_key,
            model,
            http_client,
            db,
            project_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the current project ID for usage tracking
    pub async fn set_project_id(&self, project_id: Option<i64>) {
        let mut pid = self.project_id.write().await;
        *pid = project_id;
    }

    /// Get the API key (for batch API)
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Get the model being used
    pub fn model(&self) -> EmbeddingModel {
        self.model
    }

    /// Get embedding dimensions for current model
    pub fn dimensions(&self) -> usize {
        self.model.dimensions()
    }

    /// Record embedding usage
    async fn record_usage(&self, tokens: u64, text_count: u64) {
        if let Some(ref db) = self.db {
            let project_id = *self.project_id.read().await;
            let cost = (tokens as f64 / 1_000_000.0) * self.model.cost_per_million();

            let record = EmbeddingUsageRecord {
                provider: "openai".to_string(),
                model: self.model.model_name().to_string(),
                tokens,
                text_count,
                cost_estimate: Some(cost),
                project_id,
            };

            if let Err(e) = db.insert_embedding_usage(&record) {
                tracing::warn!("Failed to record embedding usage: {}", e);
            }
        }
    }

    /// Embed a single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Truncate if too long
        let text = if text.len() > MAX_TEXT_CHARS {
            debug!("Truncating text from {} to {} chars", text.len(), MAX_TEXT_CHARS);
            &text[..MAX_TEXT_CHARS]
        } else {
            text
        };

        let body = serde_json::json!({
            "model": self.model.model_name(),
            "input": text
        });

        // Retry logic
        let mut last_error = None;
        for attempt in 0..=RETRY_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            match self
                .http_client
                .post(API_URL)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        let json: serde_json::Value = response.json().await?;

                        // Track usage
                        if let Some(usage) = json.get("usage") {
                            if let Some(tokens) = usage.get("total_tokens").and_then(|v| v.as_u64()) {
                                self.record_usage(tokens, 1).await;
                            }
                        }

                        if let Some(data) = json["data"].as_array() {
                            if let Some(first) = data.first() {
                                if let Some(values) = first["embedding"].as_array() {
                                    let embedding: Vec<f32> = values
                                        .iter()
                                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                                        .collect();
                                    if embedding.len() == self.dimensions() {
                                        return Ok(embedding);
                                    }
                                }
                            }
                        }
                        anyhow::bail!("Invalid embedding response");
                    } else {
                        let status = response.status();
                        let text = response.text().await.unwrap_or_default();
                        last_error = Some(anyhow::anyhow!("API error {}: {}", status, text));
                    }
                }
                Err(e) => {
                    last_error = Some(e.into());
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error")))
    }

    /// Embed multiple texts in batch (parallel)
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // Small batches: sequential
        if texts.len() <= 2 {
            let mut results = Vec::with_capacity(texts.len());
            for text in texts {
                results.push(self.embed(text).await?);
            }
            return Ok(results);
        }

        // Large batches: chunk into MAX_BATCH_SIZE and process in parallel
        let chunks: Vec<Vec<String>> = texts.chunks(MAX_BATCH_SIZE)
            .map(|c| c.to_vec())
            .collect();
        let num_batches = chunks.len();

        if num_batches == 1 {
            // Single batch, no need for parallelism
            return self.embed_batch_inner(&chunks[0]).await;
        }

        debug!("Embedding {} texts in {} parallel batches", texts.len(), num_batches);

        // Process batches in parallel using join_all
        let futures: Vec<_> = chunks.iter()
            .map(|chunk| self.embed_batch_inner(chunk))
            .collect();

        let results = futures::future::join_all(futures).await;

        // Collect results in order
        let mut all_results = Vec::with_capacity(texts.len());
        for result in results {
            all_results.extend(result?);
        }

        Ok(all_results)
    }

    /// Internal batch embedding
    async fn embed_batch_inner(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Truncate texts and collect into array
        let inputs: Vec<&str> = texts
            .iter()
            .map(|text| {
                if text.len() > MAX_TEXT_CHARS {
                    &text[..MAX_TEXT_CHARS]
                } else {
                    text.as_str()
                }
            })
            .collect();

        let body = serde_json::json!({
            "model": self.model.model_name(),
            "input": inputs
        });

        let response = self
            .http_client
            .post(API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .context("Batch embed request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Batch API error {}: {}", status, text);
        }

        let json: serde_json::Value = response.json().await?;

        // Track usage
        if let Some(usage) = json.get("usage") {
            if let Some(tokens) = usage.get("total_tokens").and_then(|v| v.as_u64()) {
                self.record_usage(tokens, texts.len() as u64).await;
            }
        }

        let data = json["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid batch response"))?;

        // OpenAI returns results with index field, sort by index to maintain order
        let mut indexed: Vec<(usize, Vec<f32>)> = Vec::with_capacity(data.len());
        for item in data {
            let index = item["index"].as_u64().unwrap_or(0) as usize;
            if let Some(values) = item["embedding"].as_array() {
                let vec: Vec<f32> = values
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                indexed.push((index, vec));
            }
        }
        indexed.sort_by_key(|(i, _)| *i);

        Ok(indexed.into_iter().map(|(_, v)| v).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncation() {
        let long_text = "a".repeat(10000);
        let truncated = if long_text.len() > MAX_TEXT_CHARS {
            &long_text[..MAX_TEXT_CHARS]
        } else {
            &long_text
        };
        assert_eq!(truncated.len(), MAX_TEXT_CHARS);
    }
}
