// src/llm/embeddings.rs

use anyhow::{anyhow, Context, Result};
use reqwest::Method;
use serde_json::{json, Value};

use crate::llm::client::OpenAIClient;

impl OpenAIClient {
    /// Get a single OpenAI text embedding for a string.
    /// Defaults to `text-embedding-3-large` unless `EMBEDDINGS_MODEL` is set.
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let model = std::env::var("EMBEDDINGS_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-large".to_string());

        let body = json!({
            "input": text,
            "model": model,
        });

        let res: Value = self
            .request(Method::POST, "embeddings")
            .json(&body)
            .send()
            .await
            .context("POST /v1/embeddings failed")?
            .json()
            .await
            .context("Invalid JSON from /v1/embeddings")?;

        // Shape: { data: [ { embedding: [f32; N], ... } ], ... }
        let arr = res
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow!("Embeddings response missing `data` array"))?;

        let first = arr
            .get(0)
            .ok_or_else(|| anyhow!("Embeddings response `data` array was empty"))?;

        let emb = first
            .get("embedding")
            .and_then(|e| e.as_array())
            .ok_or_else(|| anyhow!("Embeddings response missing `embedding`"))?;

        let v: Vec<f32> = emb
            .iter()
            .map(|x| x.as_f64().unwrap_or(0.0) as f32)
            .collect();

        if v.is_empty() {
            return Err(anyhow!("Embedding vector was empty"));
        }

        Ok(v)
    }

    /// Optional convenience: get embeddings for multiple inputs (same model).
    /// Keeps API surface non‑breaking (your existing calls to get_embedding stay the same).
    pub async fn get_embeddings_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }
        let model = std::env::var("EMBEDDINGS_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-large".to_string());

        let body = json!({
            "input": inputs,
            "model": model,
        });

        let res: Value = self
            .request(Method::POST, "embeddings")
            .json(&body)
            .send()
            .await
            .context("POST /v1/embeddings (batch) failed")?
            .json()
            .await
            .context("Invalid JSON from /v1/embeddings (batch)")?;

        let data = res
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow!("Embeddings (batch) response missing `data` array"))?;

        let mut out = Vec::with_capacity(data.len());
        for item in data {
            let emb = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| anyhow!("Embeddings (batch) item missing `embedding`"))?;
            let v: Vec<f32> = emb.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
            out.push(v);
        }

        Ok(out)
    }
}
