// src/llm/embeddings.rs

use crate::llm::client::OpenAIClient;
use anyhow::{Result, anyhow};
use serde_json::json;

impl OpenAIClient {
    /// Gets OpenAI text embedding (3072d) for a string.
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.api_base);
        let req_body = json!({
            "input": text,
            "model": "text-embedding-3-large"
        });
        
        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&req_body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("OpenAI embedding failed: {}", resp.text().await.unwrap_or_default()));
        }
        
        let resp_json: serde_json::Value = resp.json().await?;
        
        // Extract embedding (3072 floats)
        let embedding = resp_json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow!("No embedding in OpenAI response"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(embedding)
    }
}
