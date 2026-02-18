// crates/mira-server/src/embeddings/mod.rs
// Embedding provider module

mod ollama;
mod openai;

pub use self::ollama::OllamaEmbeddings;
pub use self::openai::{OpenAiEmbeddingModel, OpenAiEmbeddings};

use crate::config::{ApiKeys, EmbeddingsConfig};
use crate::db::pool::DatabasePool;
use anyhow::Result;
use std::sync::Arc;
use tracing::info;

/// Configuration key for storing the active embedding provider in server_state
pub const EMBEDDING_PROVIDER_KEY: &str = "embedding_provider";

/// Backend-specific embedding implementation
enum EmbeddingBackend {
    OpenAi(OpenAiEmbeddings),
    Ollama(OllamaEmbeddings),
}

/// Embedding client with automatic provider selection
///
/// Priority: OpenAI (highest quality) > Ollama (local, no key needed)
pub struct EmbeddingClient {
    backend: EmbeddingBackend,
}

impl EmbeddingClient {
    /// Provider identifier for change detection
    pub fn provider_id(&self) -> &'static str {
        match &self.backend {
            EmbeddingBackend::OpenAi(_) => "openai",
            EmbeddingBackend::Ollama(_) => "ollama",
        }
    }

    /// Create a new embedding client from pre-loaded configuration (avoids duplicate env reads)
    ///
    /// Priority: OpenAI key → Ollama host → None
    pub fn from_config(
        api_keys: &ApiKeys,
        config: &EmbeddingsConfig,
        pool: Option<Arc<DatabasePool>>,
    ) -> Option<Self> {
        // Priority 1: OpenAI (highest quality, requires API key)
        if let Some(api_key) = api_keys.openai.as_ref() {
            info!("Using OpenAI embeddings (text-embedding-3-small)");
            return Some(Self {
                backend: EmbeddingBackend::OpenAi(OpenAiEmbeddings::with_config(
                    api_key.clone(),
                    OpenAiEmbeddingModel::default(),
                    config.dimensions,
                    pool,
                )),
            });
        }

        // Priority 2: Ollama (local, no API key needed)
        if let Some(ollama_host) = api_keys.ollama.as_ref() {
            let client = OllamaEmbeddings::new(
                ollama_host.clone(),
                config.ollama_embedding_model.clone(),
                config.dimensions,
            );
            info!(
                model = client.model_name(),
                dimensions = client.dimensions(),
                "Using Ollama embeddings"
            );
            return Some(Self {
                backend: EmbeddingBackend::Ollama(client),
            });
        }

        None
    }

    /// Create a new embedding client from pre-loaded configuration with a shared HTTP client
    pub fn from_config_with_http_client(
        api_keys: &ApiKeys,
        config: &EmbeddingsConfig,
        pool: Option<Arc<DatabasePool>>,
        http_client: reqwest::Client,
    ) -> Option<Self> {
        if let Some(api_key) = api_keys.openai.as_ref() {
            return Some(Self {
                backend: EmbeddingBackend::OpenAi(OpenAiEmbeddings::with_http_client(
                    api_key.clone(),
                    OpenAiEmbeddingModel::default(),
                    config.dimensions,
                    pool,
                    http_client,
                )),
            });
        }

        // Ollama uses its own HTTP client (different timeout/config)
        if let Some(ollama_host) = api_keys.ollama.as_ref() {
            let client = OllamaEmbeddings::new(
                ollama_host.clone(),
                config.ollama_embedding_model.clone(),
                config.dimensions,
            );
            info!(
                model = client.model_name(),
                dimensions = client.dimensions(),
                "Using Ollama embeddings"
            );
            return Some(Self {
                backend: EmbeddingBackend::Ollama(client),
            });
        }

        None
    }

    /// Create a new embedding client from environment configuration
    ///
    /// Checks for OPENAI_API_KEY, then OLLAMA_HOST
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
        match &self.backend {
            EmbeddingBackend::OpenAi(c) => c.dimensions(),
            EmbeddingBackend::Ollama(c) => c.dimensions(),
        }
    }

    /// Get model name for display/logging
    pub fn model_name(&self) -> String {
        match &self.backend {
            EmbeddingBackend::OpenAi(c) => c.model().model_name().to_string(),
            EmbeddingBackend::Ollama(c) => c.model_name().to_string(),
        }
    }

    /// Provider-appropriate sub-batch size for streaming storage in the indexer.
    ///
    /// Using the provider's native batch size ensures each sub-batch maps to a
    /// single HTTP request, so a failure in one sub-batch doesn't discard
    /// embeddings from earlier sub-batches without sacrificing per-batch throughput.
    pub fn batch_size(&self) -> usize {
        match &self.backend {
            EmbeddingBackend::OpenAi(_) => 256, // matches openai::MAX_BATCH_SIZE
            EmbeddingBackend::Ollama(_) => 64,  // matches ollama::MAX_BATCH_SIZE
        }
    }

    /// Set project ID for usage tracking
    pub async fn set_project_id(&self, project_id: Option<i64>) {
        match &self.backend {
            EmbeddingBackend::OpenAi(c) => c.set_project_id(project_id).await,
            EmbeddingBackend::Ollama(_) => {} // no usage tracking for local
        }
    }

    /// Embed a single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        match &self.backend {
            EmbeddingBackend::OpenAi(c) => c.embed(text).await,
            EmbeddingBackend::Ollama(c) => c.embed(text).await,
        }
    }

    /// Embed multiple texts in batch
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        match &self.backend {
            EmbeddingBackend::OpenAi(c) => c.embed_batch(texts).await,
            EmbeddingBackend::Ollama(c) => c.embed_batch(texts).await,
        }
    }
}
