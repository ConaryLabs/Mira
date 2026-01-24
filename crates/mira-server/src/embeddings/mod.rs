// crates/mira-server/src/embeddings/mod.rs
// Google embeddings module

mod google;

pub use google::{GoogleEmbeddingModel, GoogleEmbeddings, TaskType};

use crate::db::pool::DatabasePool;
use anyhow::Result;
use std::sync::Arc;

/// Embedding client using Google Gemini embeddings
pub struct EmbeddingClient {
    inner: GoogleEmbeddings,
}

impl EmbeddingClient {
    /// Create a new embedding client from environment configuration
    ///
    /// Checks for GEMINI_API_KEY or GOOGLE_API_KEY
    pub fn from_env(pool: Option<Arc<DatabasePool>>) -> Option<Self> {
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

        Some(Self {
            inner: GoogleEmbeddings::with_config(
                api_key,
                GoogleEmbeddingModel::default(),
                dimensions,
                task_type,
                pool,
            ),
        })
    }

    /// Create a new embedding client from environment configuration with a shared HTTP client
    pub fn from_env_with_http_client(
        pool: Option<Arc<DatabasePool>>,
        http_client: reqwest::Client,
    ) -> Option<Self> {
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

        Some(Self {
            inner: GoogleEmbeddings::with_http_client(
                api_key,
                GoogleEmbeddingModel::default(),
                dimensions,
                task_type,
                pool,
                http_client,
            ),
        })
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

    /// Get the inner Google embeddings client
    pub fn inner(&self) -> &GoogleEmbeddings {
        &self.inner
    }
}

/// Configuration key for storing dimensions in server_state
pub const EMBEDDING_DIMENSIONS_KEY: &str = "embedding_dimensions";
