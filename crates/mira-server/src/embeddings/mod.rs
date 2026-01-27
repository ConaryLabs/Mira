// crates/mira-server/src/embeddings/mod.rs
// Google embeddings module

mod google;

pub use google::{GoogleEmbeddingModel, GoogleEmbeddings, TaskType};

use crate::config::{ApiKeys, EmbeddingsConfig};
use crate::db::pool::DatabasePool;
use anyhow::Result;
use std::sync::Arc;

/// Embedding client using Google Gemini embeddings
pub struct EmbeddingClient {
    inner: GoogleEmbeddings,
}

impl EmbeddingClient {
    /// Create a new embedding client from pre-loaded configuration (avoids duplicate env reads)
    pub fn from_config(
        api_keys: &ApiKeys,
        config: &EmbeddingsConfig,
        pool: Option<Arc<DatabasePool>>,
    ) -> Option<Self> {
        let api_key = api_keys.gemini.as_ref()?;

        Some(Self {
            inner: GoogleEmbeddings::with_config(
                api_key.clone(),
                GoogleEmbeddingModel::default(),
                config.dimensions,
                config.task_type.clone(),
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
        let api_key = api_keys.gemini.as_ref()?;

        Some(Self {
            inner: GoogleEmbeddings::with_http_client(
                api_key.clone(),
                GoogleEmbeddingModel::default(),
                config.dimensions,
                config.task_type.clone(),
                pool,
                http_client,
            ),
        })
    }

    /// Create a new embedding client from environment configuration
    ///
    /// Checks for GEMINI_API_KEY or GOOGLE_API_KEY
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

    /// Embed a single text using the default task type
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.inner.embed(text).await
    }

    /// Embed text optimized for document storage (RETRIEVAL_DOCUMENT)
    /// Use this when storing memories for later retrieval
    pub async fn embed_for_storage(&self, text: &str) -> Result<Vec<f32>> {
        self.inner.embed_for_storage(text).await
    }

    /// Embed text optimized for search queries (RETRIEVAL_QUERY)
    /// Use this when searching/recalling memories
    pub async fn embed_for_query(&self, text: &str) -> Result<Vec<f32>> {
        self.inner.embed_for_query(text).await
    }

    /// Embed code content (CODE_RETRIEVAL_QUERY)
    /// Use this for code indexing and semantic code search
    pub async fn embed_code(&self, text: &str) -> Result<Vec<f32>> {
        self.inner.embed_code(text).await
    }

    /// Embed multiple texts in batch using the default task type
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch(texts).await
    }

    /// Embed multiple texts optimized for document storage (RETRIEVAL_DOCUMENT)
    pub async fn embed_batch_for_storage(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch_for_storage(texts).await
    }

    /// Embed multiple texts optimized for code (CODE_RETRIEVAL_QUERY)
    pub async fn embed_batch_code(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch_code(texts).await
    }

    /// Get the inner Google embeddings client
    pub fn inner(&self) -> &GoogleEmbeddings {
        &self.inner
    }
}

/// Configuration key for storing dimensions in server_state
pub const EMBEDDING_DIMENSIONS_KEY: &str = "embedding_dimensions";
