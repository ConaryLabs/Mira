// src/llm/provider/openai.rs
// GPT-5 Responses API provider implementation with battle-tested text extraction

use super::{LlmProvider, ChatMessage, ProviderResponse, ProviderMetadata};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Instant;
use tracing::debug;

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: usize,
    reasoning_effort: String,
    verbosity: String,
}

impl OpenAiProvider {
    pub fn new(
        api_key: String,
        model: String,
        max_tokens: usize,
        reasoning_effort: String,
        verbosity: String,
    ) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            max_tokens,
            reasoning_effort: normalize_reasoning_effort(&reasoning_effort),
            verbosity: normalize_verbosity(&verbosity),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &'static str {
        "gpt5"
    }
    
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        system: String,
        _thinking: Option<u32>, // GPT-5 uses reasoning_effort instead
    ) -> Result<ProviderResponse> {
        let start = Instant::now();
        
        // Convert to GPT-5 input format (single string)
        let mut full_input = system.clone();
        for msg in messages {
            full_input.push_str(&format!("\n\n{}: {}", msg.role, msg.content));
        }
        
        let body = json!({
            "model": self.model,
            "input": full_input,
            "text": {
                "verbosity": self.verbosity,
                "format": { "type": "text" }
            },
            "reasoning": {
                "effort": self.reasoning_effort
            },
            "max_output_tokens": self.max_tokens
        });
        
        debug!("GPT-5 request: model={}, effort={}", self.model, self.reasoning_effort);
        
        let response = self.client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("GPT-5 API error {}: {}", status, error_text));
        }
        
        let raw_response = response.json::<Value>().await?;
        let latency_ms = start.elapsed().as_millis() as i64;
        
        // Extract text using battle-tested extraction
        let content = extract_text_from_responses(&raw_response)
            .ok_or_else(|| anyhow!("Failed to extract text from GPT-5 response"))?;
        
        // Extract usage metadata
        let usage = raw_response.get("usage");
        let metadata = ProviderMetadata {
            model_version: self.model.clone(),
            input_tokens: usage.and_then(|u| u["prompt_tokens"].as_i64()),
            output_tokens: usage.and_then(|u| u["completion_tokens"].as_i64()),
            thinking_tokens: usage.and_then(|u| u["reasoning_tokens"].as_i64()),
            total_tokens: usage.and_then(|u| u["total_tokens"].as_i64()),
            latency_ms,
            finish_reason: raw_response["finish_reason"].as_str().map(|s| s.to_string()),
        };
        
        Ok(ProviderResponse {
            content,
            thinking: None, // GPT-5 doesn't expose reasoning separately
            metadata,
        })
    }
    
    async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        system: String,
        tools: Vec<Value>,
        tool_choice: Option<Value>,  // NEW: Accept for API consistency
    ) -> Result<Value> {
        // Convert to GPT-5 input format
        let mut full_input = system.clone();
        for msg in messages {
            full_input.push_str(&format!("\n\n{}: {}", msg.role, msg.content));
        }
        
        let body = json!({
            "model": self.model,
            "input": full_input,
            "text": {
                "verbosity": self.verbosity,
                "format": { "type": "text" }
            },
            "reasoning": {
                "effort": self.reasoning_effort
            },
            "max_output_tokens": self.max_tokens,
            "tools": tools,
        });
        
        if tool_choice.is_some() {
            debug!("GPT-5 tool_choice requested but not yet implemented");
            // TODO: Add tool_choice support if GPT-5 API supports it
        }
        
        debug!("GPT-5 tool request: {} tools", tools.len());
        
        let response = self.client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("GPT-5 API error {}: {}", status, error_text));
        }
        
        // Return raw response (will be converted to unified format by caller)
        Ok(response.json().await?)
    }
}

/// Extract text from GPT-5 Responses API - battle-tested
fn extract_text_from_responses(response: &Value) -> Option<String> {
    // PRIMARY: output[1].content[0].text (output[0]=reasoning, output[1]=message)
    if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
        for item in output_array {
            if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                    if let Some(first_content) = content_array.first() {
                        if let Some(text) = first_content.get("text").and_then(|t| t.as_str()) {
                            return Some(text.to_string());
                        }
                    }
                }
            }
        }
    }
    
    // FALLBACK 1: Direct path /output/1/content/0/text
    if let Some(text) = response.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
        return Some(text.to_string());
    }
    
    // FALLBACK 2: Convenience field output_text
    if let Some(text) = response.get("output_text").and_then(|t| t.as_str()) {
        return Some(text.to_string());
    }
    
    // FALLBACK 3: Try output.message.content[0].text
    if let Some(text) = response.pointer("/output/message/content/0/text").and_then(|t| t.as_str()) {
        return Some(text.to_string());
    }
    
    None
}

/// Normalize verbosity to valid API values
fn normalize_verbosity(verbosity: &str) -> String {
    match verbosity.to_lowercase().as_str() {
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }.to_string()
}

/// Normalize reasoning effort to valid API values
fn normalize_reasoning_effort(effort: &str) -> String {
    match effort.to_lowercase().as_str() {
        "minimal" => "minimal",
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }.to_string()
}
