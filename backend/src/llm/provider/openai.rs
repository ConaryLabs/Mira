// src/llm/provider/openai.rs
// OpenAI provider - EMBEDDINGS ONLY
// Chat/reasoning is handled by gpt5.rs (GPT-5 Responses API)

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

pub struct OpenAiEmbeddings {
    client: Client,
    api_key: String,
    model: String,
}

impl OpenAiEmbeddings {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }
    
    /// Generate embedding for text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let body = json!({
            "model": self.model,
            "input": text,
        });
        
        let response = self.client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("OpenAI API error {}: {}", status, error_text));
        }
        
        let raw = response.json::<Value>().await?;
        let embedding = raw["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow!("No embedding in OpenAI response"))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        
        Ok(embedding)
    }
    
    /// Generate embeddings for multiple texts in a single API call (batch optimization)
    pub async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        
        let body = json!({
            "model": self.model,
            "input": texts,
        });
        
        let response = self.client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("OpenAI API error {}: {}", status, error_text));
        }
        
        let raw = response.json::<Value>().await?;
        let data_array = raw["data"]
            .as_array()
            .ok_or_else(|| anyhow!("No data array in OpenAI response"))?;
        
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
            .collect();
        
        Ok(embeddings)
    }
}
