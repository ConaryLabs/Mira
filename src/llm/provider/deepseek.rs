// src/llm/provider/deepseek.rs
// DeepSeek 3.2 Chat API provider (OpenAI-compatible)

use super::{LlmProvider, Message, Response, ToolResponse, TokenUsage, FunctionCall, ToolContext};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Instant;
use tracing::{debug, info};

pub struct DeepSeekProvider {
    client: Client,
    api_key: String,
    model: String,        // "deepseek-chat" or "deepseek-reasoner"
    max_tokens: usize,    // 64000
    temperature: f32,     // 0.7
}

impl DeepSeekProvider {
    pub fn new(
        api_key: String,
        model: String,
        max_tokens: usize,
        temperature: f32,
    ) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            max_tokens,
            temperature,
        }
    }
    
    /// Format messages for OpenAI-compatible API
    fn format_messages(&self, messages: Vec<Message>, system: String) -> Vec<Value> {
        let mut formatted = vec![json!({
            "role": "system",
            "content": system
        })];
        
        for msg in messages {
            formatted.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }
        
        formatted
    }
    
    /// Extract function calls (OpenAI format)
    fn extract_function_calls(&self, response: &Value) -> Vec<FunctionCall> {
        let mut calls = Vec::new();
        
        if let Some(message) = response["choices"][0]["message"].as_object() {
            if let Some(tool_calls) = message["tool_calls"].as_array() {
                for call in tool_calls {
                    if let Ok(args) = serde_json::from_str::<Value>(
                        call["function"]["arguments"].as_str().unwrap_or("{}")
                    ) {
                        calls.push(FunctionCall {
                            id: call["id"].as_str().unwrap_or("").to_string(),
                            name: call["function"]["name"].as_str().unwrap_or("").to_string(),
                            arguments: args,
                        });
                    }
                }
            }
        }
        
        calls
    }
    
    /// Extract token usage with cache tracking
    fn extract_tokens(&self, response: &Value) -> TokenUsage {
        let usage = &response["usage"];
        let input = usage["prompt_tokens"].as_i64().unwrap_or(0);
        let output = usage["completion_tokens"].as_i64().unwrap_or(0);
        let cached = usage["prompt_cache_hit_tokens"].as_i64().unwrap_or(0);
        
        // Log cache hits with savings calculation
        if cached > 0 {
            let cache_percent = if input > 0 {
                (cached as f64 / input as f64) * 100.0
            } else {
                0.0
            };
            info!("💰 DeepSeek cache hit: {} tokens ({:.1}% of input, ~90% cost savings)", 
                  cached, cache_percent);
        }
        
        TokenUsage {
            input,
            output,
            reasoning: usage["reasoning_tokens"].as_i64().unwrap_or(0), // For deepseek-reasoner
            cached,
        }
    }
}

#[async_trait]
impl LlmProvider for DeepSeekProvider {
    fn name(&self) -> &'static str {
        "deepseek"
    }
    
    async fn chat(&self, messages: Vec<Message>, system: String) -> Result<Response> {
        let start = Instant::now();
        let body = json!({
            "model": self.model,
            "messages": self.format_messages(messages, system),
            "max_tokens": self.max_tokens,
            "temperature": self.temperature,
        });
        
        debug!("DeepSeek request: model={}, temp={}", self.model, self.temperature);
        
        let response = self.client
            .post("https://api.deepseek.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("DeepSeek API error {}: {}", status, error_text));
        }
        
        let raw = response.json::<Value>().await?;
        let latency = start.elapsed().as_millis() as i64;
        
        // Extract content (OpenAI format)
        let content = raw["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("No content in DeepSeek response"))?
            .to_string();
        
        Ok(Response {
            content,
            model: raw["model"].as_str().unwrap_or(&self.model).to_string(),
            tokens: self.extract_tokens(&raw),
            latency_ms: latency,
        })
    }
    
    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        _context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        // DeepSeek R1 doesn't support tool calling
        if self.model == "deepseek-reasoner" {
            return Err(anyhow!("DeepSeek R1 (deepseek-reasoner) does not support tool calling. Use deepseek-chat instead."));
        }
        
        let start = Instant::now();
        let body = json!({
            "model": self.model,
            "messages": self.format_messages(messages, system),
            "max_tokens": self.max_tokens,
            "temperature": self.temperature,
            "tools": tools,
        });
        
        debug!("DeepSeek tool request: {} tools", tools.len());
        
        let response = self.client
            .post("https://api.deepseek.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("DeepSeek API error {}: {}", status, error_text));
        }
        
        let raw = response.json::<Value>().await?;
        let latency = start.elapsed().as_millis() as i64;
        
        // Extract text content
        let text_output = raw["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        
        Ok(ToolResponse {
            id: raw["id"].as_str().unwrap_or("").to_string(),
            text_output,
            function_calls: self.extract_function_calls(&raw),
            tokens: self.extract_tokens(&raw),
            latency_ms: latency,
            raw_response: raw,
        })
    }
    
    async fn stream(
        &self,
        _messages: Vec<Message>,
        _system: String,
    ) -> Result<Box<dyn futures::Stream<Item = Result<String>> + Send + Unpin>> {
        // TODO: Implement streaming for DeepSeek
        Err(anyhow!("Streaming not yet implemented for DeepSeek"))
    }
}
