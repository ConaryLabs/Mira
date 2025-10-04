// src/llm/provider/deepseek.rs
// DeepSeek Chat API provider implementation (OpenAI-compatible)

use super::{LlmProvider, ChatMessage, ProviderResponse, ProviderMetadata};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Instant;
use tracing::{debug, warn};

pub struct DeepSeekProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: usize,
}

impl DeepSeekProvider {
    pub fn new(api_key: String, model: String, max_tokens: usize) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            max_tokens,
        }
    }
}

#[async_trait]
impl LlmProvider for DeepSeekProvider {
    fn name(&self) -> &'static str {
        "deepseek"
    }
    
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        system: String,
        _thinking: Option<u32>,
    ) -> Result<ProviderResponse> {
        let start = Instant::now();
        
        // Convert to OpenAI-compatible format
        let mut api_messages = vec![
            json!({
                "role": "system",
                "content": system
            })
        ];
        
        for msg in messages {
            api_messages.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }
        
        let body = json!({
            "model": self.model,
            "messages": api_messages,
            "max_tokens": self.max_tokens,
        });
        
        debug!("DeepSeek request: model={}", self.model);
        
        let response = self.client
            .post("https://api.deepseek.com/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("DeepSeek API error {}: {}", status, error_text));
        }
        
        let raw_response = response.json::<Value>().await?;
        let latency_ms = start.elapsed().as_millis() as i64;
        
        // Extract content (OpenAI format)
        let content = raw_response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("No content in DeepSeek response"))?
            .to_string();
        
        // DeepSeek R1 exposes reasoning in reasoning_content
        let thinking = if self.model == "deepseek-reasoner" {
            raw_response["choices"][0]["message"]["reasoning_content"]
                .as_str()
                .map(String::from)
        } else {
            None
        };
        
        // Extract usage metadata
        let usage = raw_response["usage"].as_object()
            .ok_or_else(|| anyhow!("Missing usage in DeepSeek response"))?;
        
        let metadata = ProviderMetadata {
            model_version: self.model.clone(),
            input_tokens: usage["prompt_tokens"].as_i64(),
            output_tokens: usage["completion_tokens"].as_i64(),
            thinking_tokens: usage.get("reasoning_tokens").and_then(|v| v.as_i64()),
            total_tokens: usage["total_tokens"].as_i64(),
            latency_ms,
            finish_reason: raw_response["choices"][0]["finish_reason"]
                .as_str()
                .map(|s| s.to_string()),
        };
        
        Ok(ProviderResponse {
            content,
            thinking,
            metadata,
        })
    }
    
    // DeepSeek-chat supports tools, R1 doesn't
    async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        system: String,
        tools: Vec<Value>,
        tool_choice: Option<Value>,  // NEW: Accept for API consistency
    ) -> Result<Value> {
        if self.model == "deepseek-reasoner" {
            return Err(anyhow!("DeepSeek R1 does not support tool calling"));
        }
        
        if tool_choice.is_some() {
            warn!("DeepSeek does not support forced tool_choice, ignoring");
        }
        
        // Convert to OpenAI-compatible format
        let mut api_messages = vec![
            json!({
                "role": "system",
                "content": system
            })
        ];
        
        for msg in messages {
            api_messages.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }
        
        let body = json!({
            "model": self.model,
            "messages": api_messages,
            "max_tokens": self.max_tokens,
            "tools": tools,
        });
        
        debug!("DeepSeek tool request: {} tools", tools.len());
        
        let response = self.client
            .post("https://api.deepseek.com/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("DeepSeek API error {}: {}", status, error_text));
        }
        
        // Return raw response (will be converted to Claude format by caller)
        Ok(response.json().await?)
    }
}
