// src/llm/provider/claude.rs
// Claude Messages API provider implementation

use super::{LlmProvider, ChatMessage, ProviderResponse, ProviderMetadata};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Instant;
use tracing::debug;

pub struct ClaudeProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: usize,
}

impl ClaudeProvider {
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
impl LlmProvider for ClaudeProvider {
    fn name(&self) -> &'static str {
        "claude"
    }
    
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        system: String,
        thinking_budget: Option<u32>,
    ) -> Result<ProviderResponse> {
        let start = Instant::now();
        
        // Convert to Claude Messages API format
        let mut api_messages = Vec::new();
        for msg in messages {
            // Handle both string and array content
            let content = match &msg.content {
                Value::String(s) => Value::String(s.clone()),
                other => other.clone(),
            };
            
            api_messages.push(json!({
                "role": msg.role,
                "content": content
            }));
        }
        
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": api_messages,
        });
        
        // Add thinking if requested
        if let Some(budget) = thinking_budget {
            body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": budget
            });
        }
        
        debug!("Claude request: model={}, thinking={:?}", self.model, thinking_budget);
        
        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("Claude API error {}: {}", status, error_text));
        }
        
        let raw_response = response.json::<Value>().await?;
        let latency_ms = start.elapsed().as_millis() as i64;
        
        // Extract content
        let content = raw_response["content"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow!("No content in Claude response"))?
            .to_string();
        
        // Extract thinking (optional)
        let thinking = raw_response["thinking"]
            .as_str()
            .map(|s| s.to_string());
        
        // Extract metadata
        let usage = raw_response["usage"].as_object()
            .ok_or_else(|| anyhow!("Missing usage in Claude response"))?;
        
        let metadata = ProviderMetadata {
            model_version: self.model.clone(),
            input_tokens: usage["input_tokens"].as_i64(),
            output_tokens: usage["output_tokens"].as_i64(),
            thinking_tokens: usage.get("thinking_tokens").and_then(|v| v.as_i64()),
            total_tokens: Some(
                usage["input_tokens"].as_i64().unwrap_or(0) +
                usage["output_tokens"].as_i64().unwrap_or(0)
            ),
            latency_ms,
            finish_reason: raw_response["stop_reason"].as_str().map(|s| s.to_string()),
        };
        
        Ok(ProviderResponse {
            content,
            thinking,
            metadata,
        })
    }
    
    async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        system: String,
        tools: Vec<Value>,
        tool_choice: Option<Value>,
    ) -> Result<Value> {
        // Convert to Claude Messages API format
        let mut api_messages = Vec::new();
        for msg in messages {
            // Handle both string and array content
            let content = match &msg.content {
                Value::String(s) => Value::String(s.clone()),
                other => other.clone(),
            };
            
            api_messages.push(json!({
                "role": msg.role,
                "content": content
            }));
        }
        
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": api_messages,
            "tools": tools,
        });
        
        // CRITICAL: Add thinking ONLY if tool_choice is NOT set
        // Claude API doesn't allow thinking with forced tool use
        if tool_choice.is_none() {
            body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": 10000  // Default budget for tool use
            });
        }
        
        // Add tool_choice if provided (forces specific tool, disables thinking)
        if let Some(choice) = tool_choice {
            body["tool_choice"] = choice;
        }
        
        debug!(
            "Claude tool request: {} tools, forced={}", 
            tools.len(), 
            body.get("tool_choice").is_some()
        );
        
        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("Claude API error {}: {}", status, error_text));
        }
        
        // Return raw response (already in Claude format)
        Ok(response.json().await?)
    }
}
