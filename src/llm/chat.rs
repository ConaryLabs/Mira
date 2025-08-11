use crate::llm::client::OpenAIClient;
use crate::llm::schema::MiraStructuredReply;
use anyhow::{Result, anyhow};
use serde_json::json;

impl OpenAIClient {
    /// Persona-aware JSON reply via Responses API.
    /// Uses `instructions` for persona (cleaner than a system turn) and enforces JSON.
    pub async fn chat_with_custom_prompt(
        &self,
        message: &str,
        model: &str,
        system_prompt: &str,
    ) -> Result<MiraStructuredReply, anyhow::Error> {
        // Build Responses API payload
        let input = json!([
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": message }
                ]
            }
        ]);

        let body = json!({
            "model": model,                 // expected to be "gpt-5"
            "input": input,
            "instructions": system_prompt,  // persona lives here
            "response_format": { "type": "json_object" },
            "text": { "verbosity": "medium" }
        });

        let resp = self
            .request(reqwest::Method::POST, "responses")
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

        // Extract unified output text from Responses payload
        let content = crate::llm::client::extract_text_from_responses(&resp_json)
            .ok_or_else(|| anyhow!("No content in GPT-5 Responses output"))?;

        // Parse into your structured schema; if it fails, surface the raw content
        let reply: MiraStructuredReply = serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse MiraStructuredReply: {}\nRaw content:\n{}", e, content))?;

        Ok(reply)
    }

    /// Simple text chat via Responses API (no JSON schema required).
    pub async fn simple_chat(
        &self,
        message: &str,
        model: &str,
        system_prompt: &str,
    ) -> Result<String, anyhow::Error> {
        let input = json!([
            {
                "role": "system",
                "content": [
                    { "type": "input_text", "text": system_prompt }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": message }
                ]
            }
        ]);

        let body = json!({
            "model": model,  // "gpt-5"
            "input": input,
            "text": { "verbosity": "medium" }
        });

        let resp = self
            .request(reqwest::Method::POST, "responses")
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
        let content = crate::llm::client::extract_text_from_responses(&resp_json)
            .ok_or_else(|| anyhow!("No content in GPT-5 Responses output"))?;

        Ok(content)
    }

    /// New GPT-5 Responses API chat helper for structured persona prompts.
    /// Wraps text in `input_text` parts and enforces JSON; resilient to API shape.
    pub async fn chat_with_gpt5_responses(
        &self,
        message: &str,
        system_prompt: &str,
    ) -> Result<MiraStructuredReply, anyhow::Error> {
        let input = json!([
            {
                "role": "system",
                "content": [
                    { "type": "input_text", "text": system_prompt }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": message }
                ]
            }
        ]);

        let body = json!({
            "model": "gpt-5",
            "input": input,
            "response_format": { "type": "json_object" },
            "text": { "verbosity": "medium" }
        });

        let resp = self
            .request(reqwest::Method::POST, "responses")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "OpenAI chat_with_gpt5_responses failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }

        let resp_json: serde_json::Value = resp.json().await?;

        let content = crate::llm::client::extract_text_from_responses(&resp_json)
            .ok_or_else(|| anyhow!("No content in GPT-5 Responses output"))?;

        let reply: MiraStructuredReply = serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse MiraStructuredReply: {}\nRaw content:\n{}", e, content))?;

        Ok(reply)
    }
}
