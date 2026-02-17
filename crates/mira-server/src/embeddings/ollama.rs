// crates/mira-server/src/embeddings/ollama.rs
// Ollama embeddings via OpenAI-compatible /v1/embeddings endpoint

use crate::http::create_fast_client;
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;
use tracing::debug;

/// Default Ollama embedding model
const DEFAULT_MODEL: &str = "nomic-embed-text";

/// Default dimensions for nomic-embed-text
const DEFAULT_DIMENSIONS: usize = 768;

/// Max characters to embed (conservative limit for local models)
const MAX_TEXT_CHARS: usize = 8192 * 4;

/// Max texts per batch request
const MAX_BATCH_SIZE: usize = 64;

/// Retry attempts
const RETRY_ATTEMPTS: usize = 1;

/// OpenAI-compatible embedding response types (shared with openai.rs)
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    #[allow(dead_code)]
    index: usize,
}

/// Ollama embeddings client (OpenAI-compatible endpoint, no auth required)
pub struct OllamaEmbeddings {
    base_url: String,
    model: String,
    dimensions: usize,
    http_client: reqwest::Client,
}

impl OllamaEmbeddings {
    /// Create a new Ollama embeddings client
    pub fn new(base_url: String, model: Option<String>, dimensions: Option<usize>) -> Self {
        let model = model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let dimensions = dimensions.unwrap_or(DEFAULT_DIMENSIONS);
        let base_url = base_url.trim_end_matches('/').to_string();

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| create_fast_client());

        Self {
            base_url,
            model,
            dimensions,
            http_client,
        }
    }

    /// Get embedding dimensions
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Get model name
    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// Embed a single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_texts(&[text.to_string()]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Empty embedding response from Ollama"))
    }

    /// Embed multiple texts in batch
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        if texts.len() <= MAX_BATCH_SIZE {
            return self.embed_texts(texts).await;
        }

        // Process in chunks
        let mut all_results = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(MAX_BATCH_SIZE) {
            all_results.extend(self.embed_texts(chunk).await?);
        }
        Ok(all_results)
    }

    /// Core embedding call via Ollama's OpenAI-compatible endpoint
    async fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let inputs: Vec<&str> = texts
            .iter()
            .map(|t| {
                if t.len() > MAX_TEXT_CHARS {
                    debug!(
                        "Truncating text from {} to {} chars for Ollama embedding",
                        t.len(),
                        MAX_TEXT_CHARS
                    );
                    truncate_at_boundary(t, MAX_TEXT_CHARS)
                } else {
                    t.as_str()
                }
            })
            .collect();

        let input_value = if inputs.len() == 1 {
            serde_json::Value::String(inputs[0].to_string())
        } else {
            serde_json::Value::Array(
                inputs
                    .iter()
                    .map(|s| serde_json::Value::String(s.to_string()))
                    .collect(),
            )
        };

        let body = serde_json::json!({
            "input": input_value,
            "model": self.model,
        });

        let url = format!("{}/v1/embeddings", self.base_url);

        let mut last_error = None;
        for attempt in 0..=RETRY_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }

            match self
                .http_client
                .post(&url)
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
                            .context("Failed to parse Ollama embedding response")?;

                        let mut data = resp.data;
                        data.sort_by_key(|d| d.index);

                        let embeddings: Vec<Vec<f32>> =
                            data.into_iter().map(|d| d.embedding).collect();

                        // Auto-detect dimensions from first response
                        if let Some(first) = embeddings.first() {
                            if first.len() != self.dimensions {
                                debug!(
                                    "Ollama embedding dimensions: expected {}, got {} — using actual",
                                    self.dimensions,
                                    first.len()
                                );
                            }
                        }

                        return Ok(embeddings);
                    }

                    let status = response.status();
                    let body_text = response.text().await.unwrap_or_default();
                    last_error = Some(anyhow::anyhow!(
                        "Ollama embedding request failed ({}): {}",
                        status,
                        body_text
                    ));
                }
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("Ollama embedding request error: {}", e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Ollama embedding failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_dimensions() {
        let client = OllamaEmbeddings::new(
            "http://localhost:11434".to_string(),
            None,
            None,
        );
        assert_eq!(client.dimensions(), DEFAULT_DIMENSIONS);
        assert_eq!(client.model_name(), DEFAULT_MODEL);
    }

    #[test]
    fn test_custom_model_and_dimensions() {
        let client = OllamaEmbeddings::new(
            "http://localhost:11434".to_string(),
            Some("mxbai-embed-large".to_string()),
            Some(1024),
        );
        assert_eq!(client.dimensions(), 1024);
        assert_eq!(client.model_name(), "mxbai-embed-large");
    }

    #[test]
    fn test_base_url_normalization() {
        let client = OllamaEmbeddings::new(
            "http://localhost:11434/".to_string(),
            None,
            None,
        );
        assert_eq!(client.base_url, "http://localhost:11434");
    }
}
