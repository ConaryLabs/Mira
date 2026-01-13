// crates/mira-server/src/embeddings.rs
// OpenAI embeddings API client

use anyhow::{Context, Result};
use std::time::Duration;
use tracing::debug;

/// Embedding dimensions (OpenAI text-embedding-3-small)
pub const EMBEDDING_DIM: usize = 1536;

/// Model to use
const MODEL: &str = "text-embedding-3-small";

/// API endpoint
const API_URL: &str = "https://api.openai.com/v1/embeddings";

/// Max characters to embed (truncate longer text)
const MAX_TEXT_CHARS: usize = 8000;

/// Max batch size for batch embedding (OpenAI supports up to 2048)
const MAX_BATCH_SIZE: usize = 100;

/// HTTP timeout
const TIMEOUT_SECS: u64 = 30;

/// Retry attempts
const RETRY_ATTEMPTS: usize = 2;

/// Embeddings client
pub struct Embeddings {
    api_key: String,
    http_client: reqwest::Client,
}

/// Type alias for clarity
pub type EmbeddingClient = Embeddings;

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

    /// Get the API key (for batch API)
    pub fn api_key(&self) -> &str {
        &self.api_key
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
            "model": MODEL,
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
                        if let Some(data) = json["data"].as_array() {
                            if let Some(first) = data.first() {
                                if let Some(values) = first["embedding"].as_array() {
                                    let embedding: Vec<f32> = values
                                        .iter()
                                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                                        .collect();
                                    if embedding.len() == EMBEDDING_DIM {
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
            "model": MODEL,
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
