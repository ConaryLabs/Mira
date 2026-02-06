// crates/mira-server/src/embeddings/mod.rs
// Embedding provider module

mod openai;

pub use self::openai::{OpenAiEmbeddingModel, OpenAiEmbeddings};

use crate::config::{ApiKeys, EmbeddingsConfig};
use crate::db::pool::DatabasePool;
use anyhow::Result;
use std::sync::Arc;

/// Configuration key for storing the active embedding provider in server_state
pub const EMBEDDING_PROVIDER_KEY: &str = "embedding_provider";

/// Embedding client using OpenAI text-embedding-3-small
pub struct EmbeddingClient {
    inner: OpenAiEmbeddings,
}

impl EmbeddingClient {
    /// Provider identifier for change detection
    pub fn provider_id(&self) -> &'static str {
        "openai"
    }

    /// Create a new embedding client from pre-loaded configuration (avoids duplicate env reads)
    pub fn from_config(
        api_keys: &ApiKeys,
        config: &EmbeddingsConfig,
        pool: Option<Arc<DatabasePool>>,
    ) -> Option<Self> {
        let api_key = api_keys.openai.as_ref()?;

        Some(Self {
            inner: OpenAiEmbeddings::with_config(
                api_key.clone(),
                OpenAiEmbeddingModel::default(),
                config.dimensions,
                pool,
            ),
        })
    }

    /// Create a new embedding client from pre-loaded configuration with a shared HTTP client
    pub fn from_config_with_http_client(
        api_keys: &ApiKeys,
        config: &EmbeddingsConfig,
        pool: Option<Arc<DatabasePool>>,
        http_client: reqwest::Client,
    ) -> Option<Self> {
        let api_key = api_keys.openai.as_ref()?;

        Some(Self {
            inner: OpenAiEmbeddings::with_http_client(
                api_key.clone(),
                OpenAiEmbeddingModel::default(),
                config.dimensions,
                pool,
                http_client,
            ),
        })
    }

    /// Create a new embedding client from environment configuration
    ///
    /// Checks for OPENAI_API_KEY
    /// Note: Prefer from_config() to avoid duplicate env var reads
    pub fn from_env(pool: Option<Arc<DatabasePool>>) -> Option<Self> {
        Self::from_config(&ApiKeys::from_env(), &EmbeddingsConfig::from_env(), pool)
    }

    /// Create a new embedding client from environment configuration with a shared HTTP client
    /// Note: Prefer from_config_with_http_client() to avoid duplicate env var reads
    pub fn from_env_with_http_client(
        pool: Option<Arc<DatabasePool>>,
        http_client: reqwest::Client,
    ) -> Option<Self> {
        Self::from_config_with_http_client(
            &ApiKeys::from_env(),
            &EmbeddingsConfig::from_env(),
            pool,
            http_client,
        )
    }

    /// Get embedding dimensions
    pub fn dimensions(&self) -> usize {
        self.inner.dimensions()
    }

    /// Get model name for display/logging
    pub fn model_name(&self) -> String {
        self.inner.model().model_name().to_string()
    }

    /// Set project ID for usage tracking
    pub async fn set_project_id(&self, project_id: Option<i64>) {
        self.inner.set_project_id(project_id).await;
    }

    /// Embed a single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.inner.embed(text).await
    }

    /// Embed multiple texts in batch
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch(texts).await
    }

    /// Get the inner OpenAI embeddings client
    pub fn inner(&self) -> &OpenAiEmbeddings {
        &self.inner
    }
}
