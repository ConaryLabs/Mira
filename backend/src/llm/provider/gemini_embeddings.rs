// backend/src/llm/provider/gemini_embeddings.rs
// Gemini Embeddings provider using Google AI API

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, info};

/// Gemini Embeddings provider
/// Uses gemini-embedding-001 model (3072 dimensions, same as OpenAI text-embedding-3-large)
pub struct GeminiEmbeddings {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl GeminiEmbeddings {
    /// Create a new Gemini embeddings provider
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
        }
    }

    /// Build the API URL for embedding
    fn api_url(&self) -> String {
        format!(
            "{}/models/{}:embedContent?key={}",
            self.base_url, self.model, self.api_key
        )
    }

    /// Build the batch API URL for embedding multiple texts
    fn batch_api_url(&self) -> String {
        format!(
            "{}/models/{}:batchEmbedContents?key={}",
            self.base_url, self.model, self.api_key
        )
    }

    /// Generate embedding for a single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        debug!("Generating Gemini embedding for text ({} chars)", text.len());

        let body = json!({
            "content": {
                "parts": [{
                    "text": text
                }]
            }
        });

        let response = self
            .client
            .post(self.api_url())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("Gemini API error {}: {}", status, error_text));
        }

        let raw: Value = response.json().await?;

        let embedding: Vec<f32> = raw
            .get("embedding")
            .and_then(|e| e.get("values"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("No embedding values in Gemini response"))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        debug!("Generated embedding with {} dimensions", embedding.len());
        Ok(embedding)
    }

    /// Generate embeddings for multiple texts in a single API call (batch optimization)
    pub async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        info!(
            "Generating Gemini embeddings for {} texts",
            texts.len()
        );

        // Build batch request
        let requests: Vec<Value> = texts
            .iter()
            .map(|text| {
                json!({
                    "model": format!("models/{}", self.model),
                    "content": {
                        "parts": [{
                            "text": text
                        }]
                    }
                })
            })
            .collect();

        let body = json!({
            "requests": requests
        });

        let response = self
            .client
            .post(self.batch_api_url())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("Gemini API error {}: {}", status, error_text));
        }

        let raw: Value = response.json().await?;

        let embeddings_array = raw
            .get("embeddings")
            .and_then(|e| e.as_array())
            .ok_or_else(|| anyhow!("No embeddings array in Gemini batch response"))?;

        let embeddings: Vec<Vec<f32>> = embeddings_array
            .iter()
            .filter_map(|embedding| {
                embedding
                    .get("values")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                            .collect::<Vec<f32>>()
                    })
            })
            .collect();

        info!(
            "Generated {} embeddings with {} dimensions each",
            embeddings.len(),
            embeddings.first().map(|e| e.len()).unwrap_or(0)
        );

        Ok(embeddings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_url_construction() {
        let provider = GeminiEmbeddings::new(
            "test_key".to_string(),
            "gemini-embedding-001".to_string(),
        );

        let url = provider.api_url();
        assert!(url.contains("gemini-embedding-001"));
        assert!(url.contains("embedContent"));
        assert!(url.contains("key=test_key"));
    }

    #[test]
    fn test_batch_api_url_construction() {
        let provider = GeminiEmbeddings::new(
            "test_key".to_string(),
            "gemini-embedding-001".to_string(),
        );

        let url = provider.batch_api_url();
        assert!(url.contains("gemini-embedding-001"));
        assert!(url.contains("batchEmbedContents"));
        assert!(url.contains("key=test_key"));
    }
}
