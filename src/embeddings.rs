// src/embeddings.rs
// Gemini embeddings API client

use anyhow::{Context, Result};
use std::time::Duration;
use tracing::debug;

/// Embedding dimensions (Gemini text-embedding-004)
pub const EMBEDDING_DIM: usize = 3072;

/// Max characters to embed (truncate longer text)
const MAX_TEXT_CHARS: usize = 8000;

/// Max batch size for batch embedding
const MAX_BATCH_SIZE: usize = 50;

/// HTTP timeout
const TIMEOUT_SECS: u64 = 30;

/// Retry attempts
const RETRY_ATTEMPTS: usize = 2;

/// Embeddings client
pub struct Embeddings {
    api_key: String,
    http_client: reqwest::Client,
}

impl Embeddings {
    /// Create new embeddings client
    pub fn new(api_key: String) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            api_key,
            http_client,
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

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:embedContent?key={}",
            self.api_key
        );

        let body = serde_json::json!({
            "model": "models/gemini-embedding-001",
            "content": {
                "parts": [{"text": text}]
            },
            "outputDimensionality": EMBEDDING_DIM
        });

        // Retry logic
        let mut last_error = None;
        for attempt in 0..=RETRY_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            match self.http_client.post(&url).json(&body).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let json: serde_json::Value = response.json().await?;
                        if let Some(values) = json["embedding"]["values"].as_array() {
                            let embedding: Vec<f32> = values
                                .iter()
                                .filter_map(|v| v.as_f64().map(|f| f as f32))
                                .collect();
                            if embedding.len() == EMBEDDING_DIM {
                                return Ok(embedding);
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

        // Large batches: chunk and use batch API
        let mut all_results = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(MAX_BATCH_SIZE) {
            let chunk_results = self.embed_batch_inner(chunk).await?;
            all_results.extend(chunk_results);
        }

        Ok(all_results)
    }

    /// Internal batch embedding
    async fn embed_batch_inner(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:batchEmbedContents?key={}",
            self.api_key
        );

        let requests: Vec<_> = texts
            .iter()
            .map(|text| {
                let text = if text.len() > MAX_TEXT_CHARS {
                    &text[..MAX_TEXT_CHARS]
                } else {
                    text.as_str()
                };
                serde_json::json!({
                    "model": "models/gemini-embedding-001",
                    "content": {
                        "parts": [{"text": text}]
                    },
                    "outputDimensionality": EMBEDDING_DIM
                })
            })
            .collect();

        let body = serde_json::json!({ "requests": requests });

        let response = self
            .http_client
            .post(&url)
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
        let embeddings = json["embeddings"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid batch response"))?;

        let mut results = Vec::with_capacity(embeddings.len());
        for emb in embeddings {
            if let Some(values) = emb["values"].as_array() {
                let vec: Vec<f32> = values
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                results.push(vec);
            }
        }

        Ok(results)
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
