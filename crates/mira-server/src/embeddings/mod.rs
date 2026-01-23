// crates/mira-server/src/embeddings/mod.rs
// Unified embeddings module supporting multiple providers

mod google;
mod openai;

pub use google::{GoogleEmbeddingModel, GoogleEmbeddings, TaskType};
pub use openai::{EmbeddingModel, Embeddings, EMBEDDING_DIM};

use crate::db::Database;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Embedding provider selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingProvider {
    /// OpenAI embeddings (text-embedding-3-small, etc.)
    #[default]
    OpenAI,
    /// Google Gemini embeddings (gemini-embedding-001)
    Google,
}

impl EmbeddingProvider {
    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openai" => Some(Self::OpenAI),
            "google" | "gemini" => Some(Self::Google),
            _ => None,
        }
    }

    /// Get provider name
    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Google => "google",
        }
    }

    /// Get environment variable name for API key
    pub fn api_key_env(&self) -> &'static str {
        match self {
            Self::OpenAI => "OPENAI_API_KEY",
            Self::Google => "GEMINI_API_KEY", // Uses Gemini/generativelanguage API
        }
    }
}

impl std::fmt::Display for EmbeddingProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Unified embedding client that wraps either OpenAI or Google
pub enum EmbeddingClient {
    OpenAI(Embeddings),
    Google(GoogleEmbeddings),
}

impl EmbeddingClient {
    /// Create a new embedding client from environment configuration
    ///
    /// Checks MIRA_EMBEDDING_PROVIDER env var (default: openai)
    /// Then checks for the appropriate API key
    pub fn from_env(db: Option<Arc<Database>>) -> Option<Self> {
        let provider = std::env::var("MIRA_EMBEDDING_PROVIDER")
            .ok()
            .and_then(|s| EmbeddingProvider::from_str(&s))
            .unwrap_or_default();

        Self::new(provider, db)
    }

    /// Create a new embedding client from environment configuration with a shared HTTP client
    pub fn from_env_with_http_client(
        db: Option<Arc<Database>>,
        http_client: reqwest::Client,
    ) -> Option<Self> {
        let provider = std::env::var("MIRA_EMBEDDING_PROVIDER")
            .ok()
            .and_then(|s| EmbeddingProvider::from_str(&s))
            .unwrap_or_default();

        Self::new_with_http_client(provider, db, http_client)
    }

    /// Create a new embedding client for the specified provider
    pub fn new(provider: EmbeddingProvider, db: Option<Arc<Database>>) -> Option<Self> {
        match provider {
            EmbeddingProvider::OpenAI => {
                let api_key = std::env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|k| !k.trim().is_empty())?;

                // Check for model override
                let model = std::env::var("MIRA_EMBEDDING_MODEL")
                    .ok()
                    .and_then(|m| EmbeddingModel::from_name(&m))
                    .unwrap_or_default();

                Some(Self::OpenAI(Embeddings::with_model(api_key, model, db)))
            }
            EmbeddingProvider::Google => {
                // Try GEMINI_API_KEY first (Gemini/generativelanguage API), fall back to GOOGLE_API_KEY
                let api_key = std::env::var("GEMINI_API_KEY")
                    .ok()
                    .filter(|k| !k.trim().is_empty())
                    .or_else(|| std::env::var("GOOGLE_API_KEY").ok().filter(|k| !k.trim().is_empty()))?;

                // Check for dimension override (Google supports flexible dimensions)
                let dimensions = std::env::var("MIRA_EMBEDDING_DIMENSIONS")
                    .ok()
                    .and_then(|d| d.parse().ok());

                // Check for task type override
                let task_type = std::env::var("MIRA_EMBEDDING_TASK_TYPE")
                    .ok()
                    .and_then(|t| match t.to_uppercase().as_str() {
                        "SEMANTIC_SIMILARITY" => Some(TaskType::SemanticSimilarity),
                        "RETRIEVAL_DOCUMENT" => Some(TaskType::RetrievalDocument),
                        "RETRIEVAL_QUERY" => Some(TaskType::RetrievalQuery),
                        "CLASSIFICATION" => Some(TaskType::Classification),
                        "CLUSTERING" => Some(TaskType::Clustering),
                        "CODE_RETRIEVAL_QUERY" => Some(TaskType::CodeRetrievalQuery),
                        "QUESTION_ANSWERING" => Some(TaskType::QuestionAnswering),
                        "FACT_VERIFICATION" => Some(TaskType::FactVerification),
                        _ => None,
                    })
                    .unwrap_or_default();

                Some(Self::Google(GoogleEmbeddings::with_config(
                    api_key,
                    GoogleEmbeddingModel::default(),
                    dimensions,
                    task_type,
                    db,
                )))
            }
        }
    }

    /// Create a new embedding client for the specified provider with a shared HTTP client
    pub fn new_with_http_client(
        provider: EmbeddingProvider,
        db: Option<Arc<Database>>,
        http_client: reqwest::Client,
    ) -> Option<Self> {
        match provider {
            EmbeddingProvider::OpenAI => {
                let api_key = std::env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|k| !k.trim().is_empty())?;

                let model = std::env::var("MIRA_EMBEDDING_MODEL")
                    .ok()
                    .and_then(|m| EmbeddingModel::from_name(&m))
                    .unwrap_or_default();

                Some(Self::OpenAI(Embeddings::with_http_client(api_key, model, db, http_client)))
            }
            EmbeddingProvider::Google => {
                let api_key = std::env::var("GEMINI_API_KEY")
                    .ok()
                    .filter(|k| !k.trim().is_empty())
                    .or_else(|| std::env::var("GOOGLE_API_KEY").ok().filter(|k| !k.trim().is_empty()))?;

                let dimensions = std::env::var("MIRA_EMBEDDING_DIMENSIONS")
                    .ok()
                    .and_then(|d| d.parse().ok());

                let task_type = std::env::var("MIRA_EMBEDDING_TASK_TYPE")
                    .ok()
                    .and_then(|t| match t.to_uppercase().as_str() {
                        "SEMANTIC_SIMILARITY" => Some(TaskType::SemanticSimilarity),
                        "RETRIEVAL_DOCUMENT" => Some(TaskType::RetrievalDocument),
                        "RETRIEVAL_QUERY" => Some(TaskType::RetrievalQuery),
                        "CLASSIFICATION" => Some(TaskType::Classification),
                        "CLUSTERING" => Some(TaskType::Clustering),
                        "CODE_RETRIEVAL_QUERY" => Some(TaskType::CodeRetrievalQuery),
                        "QUESTION_ANSWERING" => Some(TaskType::QuestionAnswering),
                        "FACT_VERIFICATION" => Some(TaskType::FactVerification),
                        _ => None,
                    })
                    .unwrap_or_default();

                Some(Self::Google(GoogleEmbeddings::with_http_client(
                    api_key,
                    GoogleEmbeddingModel::default(),
                    dimensions,
                    task_type,
                    db,
                    http_client,
                )))
            }
        }
    }

    /// Get the provider being used
    pub fn provider(&self) -> EmbeddingProvider {
        match self {
            Self::OpenAI(_) => EmbeddingProvider::OpenAI,
            Self::Google(_) => EmbeddingProvider::Google,
        }
    }

    /// Get embedding dimensions
    pub fn dimensions(&self) -> usize {
        match self {
            Self::OpenAI(e) => e.dimensions(),
            Self::Google(e) => e.dimensions(),
        }
    }

    /// Get model name for display/logging
    pub fn model_name(&self) -> String {
        match self {
            Self::OpenAI(e) => e.model().model_name().to_string(),
            Self::Google(e) => e.model().model_name().to_string(),
        }
    }

    /// Set project ID for usage tracking
    pub async fn set_project_id(&self, project_id: Option<i64>) {
        match self {
            Self::OpenAI(e) => e.set_project_id(project_id).await,
            Self::Google(e) => e.set_project_id(project_id).await,
        }
    }

    /// Embed a single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        match self {
            Self::OpenAI(e) => e.embed(text).await,
            Self::Google(e) => e.embed(text).await,
        }
    }

    /// Embed multiple texts in batch
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        match self {
            Self::OpenAI(e) => e.embed_batch(texts).await,
            Self::Google(e) => e.embed_batch(texts).await,
        }
    }
}

/// Configuration key for storing provider in server_state
pub const EMBEDDING_PROVIDER_KEY: &str = "embedding_provider";

/// Configuration key for storing dimensions in server_state
pub const EMBEDDING_DIMENSIONS_KEY: &str = "embedding_dimensions";
