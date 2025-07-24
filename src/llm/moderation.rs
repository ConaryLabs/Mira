// src/llm/moderation.rs

use crate::llm::client::OpenAIClient;
use anyhow::Result;
use serde_json::json;

#[derive(Debug)]
pub struct ModerationResult {
    pub flagged: bool,
    pub categories: Vec<String>,
}

impl OpenAIClient {
    /// Runs moderation API on user/Mira message, returns flagged true/false.
    /// *Log-only version for private use—never blocks!*
    pub async fn moderate(&self, text: &str) -> Result<Option<ModerationResult>> {
        let url = format!("{}/moderations", self.api_base);
        let req_body = json!({ "input": text });
        
        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&req_body)
            .send()
            .await?;
            
        if !resp.status().is_success() {
            tracing::warn!("OpenAI moderation call failed: {}", resp.text().await.unwrap_or_default());
            return Ok(None);
        }
        
        let resp_json: serde_json::Value = resp.json().await?;
        let flagged = resp_json["results"][0]["flagged"].as_bool().unwrap_or(false);
        let categories = resp_json["results"][0]["categories"]
            .as_object()
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_else(Vec::new);

        if flagged {
            tracing::warn!("MODERATION (WARN ONLY): flagged categories: {:?}", categories);
        }
        
        Ok(Some(ModerationResult { flagged, categories }))
    }
}
