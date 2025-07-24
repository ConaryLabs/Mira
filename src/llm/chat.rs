// src/llm/chat.rs

use crate::llm::client::OpenAIClient;
use crate::llm::schema::MiraStructuredReply;
use anyhow::{Result, anyhow};
use serde_json::json;

impl OpenAIClient {
    /// Chat with model using a custom system prompt (for persona-aware responses)
    /// This version enforces JSON output format
    pub async fn chat_with_custom_prompt(
        &self, 
        message: &str, 
        model: &str,
        system_prompt: &str
    ) -> Result<MiraStructuredReply, anyhow::Error> {
        let url = format!("{}/chat/completions", self.api_base);

        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": message}),
        ];

        let body = json!({
            "model": model,
            "messages": messages,
            "temperature": 0.8,
            "response_format": { "type": "json_object" }
        });

        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "OpenAI chat_with_custom_prompt failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        
        let resp_json: serde_json::Value = resp.json().await?;

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("No content in OpenAI chat response"))?;

        let reply: MiraStructuredReply = serde_json::from_str(content)
            .map_err(|e| anyhow!("Failed to parse MiraStructuredReply: {}\nRaw content:\n{}", e, content))?;

        Ok(reply)
    }

    /// Simple chat method for utility functions that returns plain text
    /// Does NOT enforce JSON format
    pub async fn simple_chat(
        &self,
        message: &str,
        model: &str,
        system_prompt: &str
    ) -> Result<String, anyhow::Error> {
        let url = format!("{}/chat/completions", self.api_base);

        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": message}),
        ];

        let body = json!({
            "model": model,
            "messages": messages,
            "temperature": 0.2,  // Lower temperature for utility functions
            // NO response_format here!
        });

        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "OpenAI simple_chat failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }

        let resp_json: serde_json::Value = resp.json().await?;

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("No content in OpenAI chat response"))?
            .to_string();

        Ok(content)
    }
}
