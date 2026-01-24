// crates/mira-server/src/embeddings/google.rs
// Google Gemini embeddings API client

use crate::db::pool::DatabasePool;
use crate::db::{EmbeddingUsageRecord, insert_embedding_usage_sync};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::debug;

/// API endpoint for Gemini embeddings
const API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Max input tokens (Google limit)
const MAX_INPUT_TOKENS: usize = 2048;

/// Approximate chars per token (conservative estimate)
const CHARS_PER_TOKEN: usize = 4;

/// Max characters to embed (based on token limit)
const MAX_TEXT_CHARS: usize = MAX_INPUT_TOKENS * CHARS_PER_TOKEN;

/// Max batch size for batch embedding
const MAX_BATCH_SIZE: usize = 100;

/// HTTP timeout
const TIMEOUT_SECS: u64 = 30;

/// Retry attempts
const RETRY_ATTEMPTS: usize = 2;

/// Google embedding models
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum GoogleEmbeddingModel {
    /// gemini-embedding-001: Flexible dimensions (768-3072), $0.15/1M tokens
    #[default]
    GeminiEmbedding001,
}

impl GoogleEmbeddingModel {
    /// Get the model name for API calls
    pub fn model_name(&self) -> &'static str {
        match self {
            Self::GeminiEmbedding001 => "gemini-embedding-001",
        }
    }

    /// Get default embedding dimensions for this model
    /// Google supports flexible dimensions: 768, 1536, or 3072 recommended
    pub fn default_dimensions(&self) -> usize {
        match self {
            Self::GeminiEmbedding001 => 768, // Use 768 for efficiency, can be configured
        }
    }

    /// Get cost per million tokens
    pub fn cost_per_million(&self) -> f64 {
        match self {
            Self::GeminiEmbedding001 => 0.15,
        }
    }

    /// Parse from model name string
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "gemini-embedding-001" => Some(Self::GeminiEmbedding001),
            _ => None,
        }
    }
}

impl std::fmt::Display for GoogleEmbeddingModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.model_name())
    }
}

/// Task type for embedding optimization
/// Different task types optimize the embedding for specific use cases
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskType {
    /// General semantic similarity
    #[default]
    SemanticSimilarity,
    /// Optimized for document retrieval (use for indexed documents)
    RetrievalDocument,
    /// Optimized for query retrieval (use for search queries)
    RetrievalQuery,
    /// Text classification
    Classification,
    /// Clustering similar texts
    Clustering,
    /// Code retrieval queries
    CodeRetrievalQuery,
    /// Question answering
    QuestionAnswering,
    /// Fact verification
    FactVerification,
}

impl TaskType {
    /// Get the task type string for API calls
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SemanticSimilarity => "SEMANTIC_SIMILARITY",
            Self::RetrievalDocument => "RETRIEVAL_DOCUMENT",
            Self::RetrievalQuery => "RETRIEVAL_QUERY",
            Self::Classification => "CLASSIFICATION",
            Self::Clustering => "CLUSTERING",
            Self::CodeRetrievalQuery => "CODE_RETRIEVAL_QUERY",
            Self::QuestionAnswering => "QUESTION_ANSWERING",
            Self::FactVerification => "FACT_VERIFICATION",
        }
    }
}

/// Google Gemini embeddings client
pub struct GoogleEmbeddings {
    api_key: String,
    model: GoogleEmbeddingModel,
    dimensions: usize,
    task_type: TaskType,
    http_client: reqwest::Client,
    pool: Option<Arc<DatabasePool>>,
    project_id: Arc<RwLock<Option<i64>>>,
}

impl GoogleEmbeddings {
    /// Create new Google embeddings client with default settings
    pub fn new(api_key: String) -> Self {
        Self::with_config(api_key, GoogleEmbeddingModel::default(), None, TaskType::default(), None)
    }

    /// Create embeddings client with database pool for usage tracking
    pub fn with_pool(api_key: String, pool: Option<Arc<DatabasePool>>) -> Self {
        Self::with_config(api_key, GoogleEmbeddingModel::default(), None, TaskType::default(), pool)
    }

    /// Create embeddings client with full configuration
    pub fn with_config(
        api_key: String,
        model: GoogleEmbeddingModel,
        dimensions: Option<usize>,
        task_type: TaskType,
        pool: Option<Arc<DatabasePool>>,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self::with_http_client(api_key, model, dimensions, task_type, pool, http_client)
    }

    /// Create embeddings client with a shared HTTP client
    pub fn with_http_client(
        api_key: String,
        model: GoogleEmbeddingModel,
        dimensions: Option<usize>,
        task_type: TaskType,
        pool: Option<Arc<DatabasePool>>,
        http_client: reqwest::Client,
    ) -> Self {
        let dimensions = dimensions.unwrap_or_else(|| model.default_dimensions());

        Self {
            api_key,
            model,
            dimensions,
            task_type,
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
    pub fn model(&self) -> GoogleEmbeddingModel {
        self.model
    }

    /// Get the task type
    pub fn task_type(&self) -> TaskType {
        self.task_type
    }

    /// Record embedding usage
    async fn record_usage(&self, tokens: u64, text_count: u64) {
        if let Some(ref pool) = self.pool {
            let project_id = *self.project_id.read().await;
            let cost = (tokens as f64 / 1_000_000.0) * self.model.cost_per_million();

            let record = EmbeddingUsageRecord {
                provider: "google".to_string(),
                model: self.model.model_name().to_string(),
                tokens,
                text_count,
                cost_estimate: Some(cost),
                project_id,
            };

            if let Err(e) = pool.interact(move |conn| {
                insert_embedding_usage_sync(conn, &record)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            }).await {
                tracing::warn!("Failed to record embedding usage: {}", e);
            }
        }
    }

    /// Estimate token count from text (conservative)
    fn estimate_tokens(text: &str) -> u64 {
        (text.len() / CHARS_PER_TOKEN) as u64 + 1
    }

    /// Build the API URL for the model
    fn api_url(&self) -> String {
        format!("{}/{}:embedContent", API_URL, self.model.model_name())
    }

    /// Build the batch API URL for the model
    fn batch_api_url(&self) -> String {
        format!("{}/{}:batchEmbedContents", API_URL, self.model.model_name())
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
            "model": format!("models/{}", self.model.model_name()),
            "content": {
                "parts": [{"text": text}]
            },
            "taskType": self.task_type.as_str(),
            "outputDimensionality": self.dimensions
        });

        // Retry logic
        let mut last_error = None;
        for attempt in 0..=RETRY_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }

            match self
                .http_client
                .post(&self.api_url())
                .header("x-goog-api-key", &self.api_key)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        let json: serde_json::Value = response.json().await?;

                        // Track usage (estimate tokens since Google doesn't return usage)
                        let estimated_tokens = Self::estimate_tokens(text);
                        self.record_usage(estimated_tokens, 1).await;

                        // Extract embedding from response
                        if let Some(embedding) = json["embedding"]["values"].as_array() {
                            let values: Vec<f32> = embedding
                                .iter()
                                .filter_map(|v| v.as_f64().map(|f| f as f32))
                                .collect();

                            if values.len() == self.dimensions {
                                return Ok(values);
                            } else {
                                anyhow::bail!(
                                    "Dimension mismatch: expected {}, got {}",
                                    self.dimensions,
                                    values.len()
                                );
                            }
                        }
                        anyhow::bail!("Invalid embedding response format");
                    } else {
                        let status = response.status();
                        let error_text = response.text().await.unwrap_or_default();
                        last_error = Some(anyhow::anyhow!("API error {}: {}", status, error_text));
                    }
                }
                Err(e) => {
                    last_error = Some(e.into());
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error")))
    }

    /// Embed multiple texts in batch
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
        let chunks: Vec<Vec<String>> = texts
            .chunks(MAX_BATCH_SIZE)
            .map(|c| c.to_vec())
            .collect();
        let num_batches = chunks.len();

        if num_batches == 1 {
            return self.embed_batch_inner(&chunks[0]).await;
        }

        debug!("Embedding {} texts in {} parallel batches", texts.len(), num_batches);

        // Process batches in parallel
        let futures: Vec<_> = chunks
            .iter()
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

    /// Internal batch embedding using Google's batchEmbedContents
    async fn embed_batch_inner(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Build requests array
        let requests: Vec<serde_json::Value> = texts
            .iter()
            .map(|text| {
                let truncated = if text.len() > MAX_TEXT_CHARS {
                    &text[..MAX_TEXT_CHARS]
                } else {
                    text.as_str()
                };
                serde_json::json!({
                    "model": format!("models/{}", self.model.model_name()),
                    "content": {
                        "parts": [{"text": truncated}]
                    },
                    "taskType": self.task_type.as_str(),
                    "outputDimensionality": self.dimensions
                })
            })
            .collect();

        let body = serde_json::json!({
            "requests": requests
        });

        let response = self
            .http_client
            .post(&self.batch_api_url())
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Batch embed request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Batch API error {}: {}", status, error_text);
        }

        let json: serde_json::Value = response.json().await?;

        // Track usage (estimate tokens)
        let total_tokens: u64 = texts.iter().map(|t| Self::estimate_tokens(t)).sum();
        self.record_usage(total_tokens, texts.len() as u64).await;

        // Extract embeddings from response
        let embeddings = json["embeddings"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid batch response format"))?;

        let mut results = Vec::with_capacity(texts.len());
        for embedding in embeddings {
            if let Some(values) = embedding["values"].as_array() {
                let vec: Vec<f32> = values
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                results.push(vec);
            } else {
                anyhow::bail!("Missing values in embedding response");
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_type_serialization() {
        assert_eq!(TaskType::RetrievalDocument.as_str(), "RETRIEVAL_DOCUMENT");
        assert_eq!(TaskType::SemanticSimilarity.as_str(), "SEMANTIC_SIMILARITY");
    }

    #[test]
    fn test_model_dimensions() {
        let model = GoogleEmbeddingModel::GeminiEmbedding001;
        assert_eq!(model.default_dimensions(), 768);
        assert_eq!(model.cost_per_million(), 0.15);
    }

    #[test]
    fn test_token_estimation() {
        // 100 chars should be ~25 tokens
        let text = "a".repeat(100);
        let tokens = GoogleEmbeddings::estimate_tokens(&text);
        assert_eq!(tokens, 26); // 100/4 + 1
    }
}
