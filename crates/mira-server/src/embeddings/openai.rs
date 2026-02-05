// crates/mira-server/src/embeddings/openai.rs
// OpenAI embeddings API client (text-embedding-3-small)

use crate::db::pool::DatabasePool;
use crate::db::{EmbeddingUsageRecord, insert_embedding_usage_sync};
use crate::http::create_fast_client;
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::debug;

/// API endpoint for OpenAI embeddings
const API_URL: &str = "https://api.openai.com/v1/embeddings";

/// Max input tokens (OpenAI limit for embedding models)
const MAX_INPUT_TOKENS: usize = 8192;

/// Approximate chars per token (conservative estimate)
const CHARS_PER_TOKEN: usize = 4;

/// Max characters to embed (based on token limit)
const MAX_TEXT_CHARS: usize = MAX_INPUT_TOKENS * CHARS_PER_TOKEN;

/// Max texts per batch request (OpenAI allows up to 2048 inputs,
/// but we cap lower to stay well within the 300k total token limit)
const MAX_BATCH_SIZE: usize = 256;

/// Retry attempts
const RETRY_ATTEMPTS: usize = 2;

/// OpenAI embedding models
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OpenAiEmbeddingModel {
    /// text-embedding-3-small: 1536 default dims, $0.02/1M tokens
    #[default]
    TextEmbedding3Small,
    /// text-embedding-3-large: 3072 default dims, $0.13/1M tokens
    TextEmbedding3Large,
}

impl OpenAiEmbeddingModel {
    /// Get the model name for API calls
    pub fn model_name(&self) -> &'static str {
        match self {
            Self::TextEmbedding3Small => "text-embedding-3-small",
            Self::TextEmbedding3Large => "text-embedding-3-large",
        }
    }

    /// Get default embedding dimensions for this model
    pub fn default_dimensions(&self) -> usize {
        match self {
            Self::TextEmbedding3Small => 1536,
            Self::TextEmbedding3Large => 3072,
        }
    }

    /// Get cost per million tokens
    pub fn cost_per_million(&self) -> f64 {
        match self {
            Self::TextEmbedding3Small => 0.02,
            Self::TextEmbedding3Large => 0.13,
        }
    }

    /// Parse from model name string
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "text-embedding-3-small" => Some(Self::TextEmbedding3Small),
            "text-embedding-3-large" => Some(Self::TextEmbedding3Large),
            _ => None,
        }
    }
}

impl std::fmt::Display for OpenAiEmbeddingModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.model_name())
    }
}

/// OpenAI embeddings response types
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
    usage: EmbeddingUsage,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    #[allow(dead_code)]
    index: usize,
}

#[derive(Debug, Deserialize)]
struct EmbeddingUsage {
    #[allow(dead_code)]
    prompt_tokens: u64,
    total_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    message: String,
    #[allow(dead_code)]
    r#type: Option<String>,
}

/// OpenAI embeddings client
pub struct OpenAiEmbeddings {
    api_key: String,
    model: OpenAiEmbeddingModel,
    dimensions: usize,
    http_client: reqwest::Client,
    pool: Option<Arc<DatabasePool>>,
    project_id: Arc<RwLock<Option<i64>>>,
}

impl OpenAiEmbeddings {
    /// Create new OpenAI embeddings client with default settings
    pub fn new(api_key: String) -> Self {
        Self::with_config(api_key, OpenAiEmbeddingModel::default(), None, None)
    }

    /// Create embeddings client with database pool for usage tracking
    pub fn with_pool(api_key: String, pool: Option<Arc<DatabasePool>>) -> Self {
        Self::with_config(api_key, OpenAiEmbeddingModel::default(), None, pool)
    }

    /// Create embeddings client with full configuration
    pub fn with_config(
        api_key: String,
        model: OpenAiEmbeddingModel,
        dimensions: Option<usize>,
        pool: Option<Arc<DatabasePool>>,
    ) -> Self {
        Self::with_http_client(api_key, model, dimensions, pool, create_fast_client())
    }

    /// Create embeddings client with a shared HTTP client
    pub fn with_http_client(
        api_key: String,
        model: OpenAiEmbeddingModel,
        dimensions: Option<usize>,
        pool: Option<Arc<DatabasePool>>,
        http_client: reqwest::Client,
    ) -> Self {
        let dimensions = dimensions.unwrap_or_else(|| model.default_dimensions());

        Self {
            api_key,
            model,
            dimensions,
            http_client,
            pool,
            project_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the current project ID for usage tracking
    pub async fn set_project_id(&self, project_id: Option<i64>) {
        let mut pid = self.project_id.write().await;
        *pid = project_id;
    }

    /// Get embedding dimensions
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Get the model being used
    pub fn model(&self) -> OpenAiEmbeddingModel {
        self.model
    }

    /// Record embedding usage
    async fn record_usage(&self, tokens: u64, text_count: u64) {
        if let Some(ref pool) = self.pool {
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

            if let Err(e) = pool
                .interact(move |conn| {
                    insert_embedding_usage_sync(conn, &record).map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
            {
                tracing::warn!("Failed to record embedding usage: {}", e);
            }
        }
    }

    /// Embed a single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_texts(&[text.to_string()]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Empty embedding response"))
    }

    /// Embed multiple texts in batch
    ///
    /// OpenAI natively supports batch embedding — pass an array of strings
    /// and get back an array of embeddings in one request.
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // For large batches, chunk to stay within limits
        if texts.len() <= MAX_BATCH_SIZE {
            return self.embed_texts(texts).await;
        }

        let chunks: Vec<&[String]> = texts.chunks(MAX_BATCH_SIZE).collect();
        let num_batches = chunks.len();

        debug!(
            "Embedding {} texts in {} parallel batches",
            texts.len(),
            num_batches
        );

        let futures: Vec<_> = chunks
            .into_iter()
            .map(|chunk| self.embed_texts(chunk))
            .collect();

        let results = futures::future::join_all(futures).await;

        let mut all_results = Vec::with_capacity(texts.len());
        for result in results {
            all_results.extend(result?);
        }

        Ok(all_results)
    }

    /// Core embedding call — handles single and batch via the same endpoint
    async fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Truncate texts that exceed the limit
        let inputs: Vec<&str> = texts
            .iter()
            .map(|t| {
                if t.len() > MAX_TEXT_CHARS {
                    debug!("Truncating text from {} to {} chars", t.len(), MAX_TEXT_CHARS);
                    truncate_at_boundary(t, MAX_TEXT_CHARS)
                } else {
                    t.as_str()
                }
            })
            .collect();

        // Build request body — single string or array
        let input_value = if inputs.len() == 1 {
            serde_json::Value::String(inputs[0].to_string())
        } else {
            serde_json::Value::Array(
                inputs.iter().map(|s| serde_json::Value::String(s.to_string())).collect(),
            )
        };

        let body = serde_json::json!({
            "input": input_value,
            "model": self.model.model_name(),
            "dimensions": self.dimensions,
            "encoding_format": "float"
        });

        // Retry logic
        let mut last_error = None;
        for attempt in 0..=RETRY_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }

            match self
                .http_client
                .post(API_URL)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        let resp: EmbeddingResponse = response
                            .json()
                            .await
                            .context("Failed to parse embedding response")?;

                        // Track actual usage (OpenAI returns real token counts)
                        self.record_usage(resp.usage.total_tokens, texts.len() as u64)
                            .await;

                        // Sort by index to ensure correct ordering
                        let mut data = resp.data;
                        data.sort_by_key(|d| d.index);

                        let embeddings: Vec<Vec<f32>> =
                            data.into_iter().map(|d| d.embedding).collect();

                        // Validate dimensions on first result
                        if let Some(first) = embeddings.first() {
                            if first.len() != self.dimensions {
                                anyhow::bail!(
                                    "Dimension mismatch: expected {}, got {}",
                                    self.dimensions,
                                    first.len()
                                );
                            }
                        }

                        return Ok(embeddings);
                    } else {
                        let status = response.status();
                        let error_text = response.text().await.unwrap_or_default();

                        // Try to parse structured error
                        let msg = serde_json::from_str::<ErrorResponse>(&error_text)
                            .map(|e| e.error.message)
                            .unwrap_or(error_text);

                        last_error = Some(anyhow::anyhow!("OpenAI API error {}: {}", status, msg));
                    }
                }
                Err(e) => {
                    last_error = Some(e.into());
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_dimensions() {
        let model = OpenAiEmbeddingModel::TextEmbedding3Small;
        assert_eq!(model.default_dimensions(), 1536);
        assert_eq!(model.cost_per_million(), 0.02);
        assert_eq!(model.model_name(), "text-embedding-3-small");
    }

    #[test]
    fn test_model_large() {
        let model = OpenAiEmbeddingModel::TextEmbedding3Large;
        assert_eq!(model.default_dimensions(), 3072);
        assert_eq!(model.cost_per_million(), 0.13);
    }

    #[test]
    fn test_model_from_name() {
        assert_eq!(
            OpenAiEmbeddingModel::from_name("text-embedding-3-small"),
            Some(OpenAiEmbeddingModel::TextEmbedding3Small)
        );
        assert_eq!(
            OpenAiEmbeddingModel::from_name("text-embedding-3-large"),
            Some(OpenAiEmbeddingModel::TextEmbedding3Large)
        );
        assert_eq!(OpenAiEmbeddingModel::from_name("unknown"), None);
    }

    #[test]
    fn test_max_text_chars() {
        // 8192 tokens * 4 chars/token = 32768 chars
        assert_eq!(MAX_TEXT_CHARS, 32768);
    }
}
