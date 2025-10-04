// src/llm/client/embedding.rs
use anyhow::Result;
use reqwest::{header, Client};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::{debug, warn};

use crate::config::CONFIG;
use super::config::ClientConfig;

pub struct EmbeddingClient {
    client: Client,
    config: ClientConfig,
}

impl EmbeddingClient {
    pub fn new(config: ClientConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        if text.is_empty() {
            return Err(anyhow::anyhow!("Cannot embed empty text"));
        }

        debug!("Generating embedding for text of length {}", text.len());

        let request_body = json!({
            "input": text,
            "model": self.config.model,
            "encoding_format": "float"
        });

        let response = self
            .client
            .post(&format!("{}/v1/embeddings", self.config.base_url))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", 
                CONFIG.get_openai_key().expect("OPENAI_API_KEY required for embeddings")))
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            warn!("Embedding request failed: {} - {}", status, error_text);
            return Err(anyhow::anyhow!("Embedding request failed: {}", status));
        }

        let response_json: Value = response.json().await?;

        let embedding = response_json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No embedding in response"))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Vec<f32>>();

        if embedding.is_empty() {
            return Err(anyhow::anyhow!("Empty embedding returned"));
        }

        debug!("Generated embedding with {} dimensions", embedding.len());
        Ok(embedding)
    }

    pub async fn get_batch_embeddings(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        debug!("Generating batch embeddings for {} texts", texts.len());

        let request_body = json!({
            "input": texts,
            "model": self.config.model,
            "encoding_format": "float"
        });

        let response = self
            .client
            .post(&format!("{}/v1/embeddings", self.config.base_url))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", 
                CONFIG.get_openai_key().expect("OPENAI_API_KEY required for embeddings")))
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            warn!("Batch embedding request failed: {} - {}", status, error_text);
            return Err(anyhow::anyhow!("Batch embedding request failed: {}", status));
        }

        let response_json: Value = response.json().await?;

        let data_array = response_json["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No data array in response"))?;

        let embeddings = data_array
            .iter()
            .filter_map(|item| {
                item["embedding"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                            .collect::<Vec<f32>>()
                    })
            })
            .collect::<Vec<Vec<f32>>>();

        debug!("Generated {} batch embeddings", embeddings.len());
        Ok(embeddings)
    }
}
