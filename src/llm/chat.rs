// src/llm/chat.rs
// Chat methods for OpenAI client using GPT-5 Responses API with correct parameter structure

use crate::llm::client::OpenAIClient;
use crate::llm::schema::MiraStructuredReply;
use anyhow::{Result, anyhow};
use serde_json::json;

impl OpenAIClient {
    /// Persona-aware JSON reply via Responses API
    pub async fn chat_with_custom_prompt(
        &self,
        message: &str,
        model: &str,
        system_prompt: &str,
    ) -> Result<MiraStructuredReply, anyhow::Error> {
        let input = json!([
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": message }
                ]
            }
        ]);

        let body = json!({
            "model": model,
            "input": input,
            "instructions": system_prompt,
            "max_output_tokens": 128000,
            "text": { 
                "format": {
                    "type": "json_object"
                },
                "verbosity": "medium" 
            },
            "reasoning": {
                "effort": "medium"
            }
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

        let content = crate::llm::client::extract_text_from_responses(&resp_json)
            .ok_or_else(|| anyhow!("No content in GPT-5 Responses output"))?;

        let reply: MiraStructuredReply = serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse MiraStructuredReply: {}\nRaw content:\n{}", e, content))?;

        Ok(reply)
    }

    /// Simple text chat via Responses API
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
            "model": model,
            "input": input,
            "max_output_tokens": 128000,
            "text": { 
                "verbosity": "medium" 
            },
            "reasoning": {
                "effort": "medium"
            }
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

    /// GPT-5 Responses API chat helper for structured persona prompts
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
            "max_output_tokens": 128000,
            "text": { 
                "format": {
                    "type": "json_object"
                },
                "verbosity": "medium" 
            },
            "reasoning": {
                "effort": "medium"
            }
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
