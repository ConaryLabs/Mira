// src/llm/provider/claude.rs
// Claude Messages API provider implementation with prompt caching + context management

use super::{LlmProvider, ChatMessage, ProviderResponse, ProviderMetadata};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Instant;
use tracing::{debug, info};

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
        
        // FIXED: System as structured blocks with 1-hour cache
        // Prompt caching is now GA - no beta header needed for 1-hour cache
        let system_blocks = vec![
            json!({
                "type": "text",
                "text": system,
                "cache_control": {"type": "ephemeral"}  // 1-hour cache
            })
        ];
        
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system_blocks,  // Cached system prompt
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
            .header("anthropic-beta", "context-management-2025-06-27")  // Auto-clear stale tools
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
        
        // ADDED: Log cache performance
        if let Some(usage) = raw_response["usage"].as_object() {
            let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let cache_write = usage.get("cache_creation_input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let input_tokens = usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            
            if cache_read > 0 {
                let cache_percent = if input_tokens > 0 {
                    (cache_read as f64 / input_tokens as f64) * 100.0
                } else {
                    0.0
                };
                info!("🎯 CACHE HIT: {} tokens read from cache ({:.1}% of input)", 
                      cache_read, cache_percent);
            }
            
            if cache_write > 0 {
                info!("💾 CACHE WRITE: {} tokens written to cache", cache_write);
            }
        }
        
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
        
        // FIXED: System blocks with 1-hour cache
        let system_blocks = vec![
            json!({
                "type": "text",
                "text": system,
                "cache_control": {"type": "ephemeral"}
            })
        ];
        
        // FIXED: Tools with 1-hour cache on LAST tool
        // Anthropic caches everything UP TO and INCLUDING the cache breakpoint
        let mut cached_tools = tools;
        if let Some(last_tool) = cached_tools.last_mut() {
            last_tool["cache_control"] = json!({"type": "ephemeral"});
        }
        
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system_blocks,  // Cached
            "messages": api_messages,
            "tools": cached_tools,    // Last tool cached
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
        
        // ADDED: Context management for auto-clearing old tool results
        // When conversation reaches 150K tokens, automatically clear old tool results
        body["context_management"] = json!({
            "edits": [{
                "type": "clear_tool_uses_20250919",
                "trigger": {"type": "input_tokens", "value": 150000},  // Trigger at 150K tokens
                "keep": {"type": "tool_uses", "value": 10},  // Keep last 10 tool results
                "clear_at_least": {"type": "input_tokens", "value": 10000}  // Clear at least 10K tokens
            }]
        });
        
        debug!(
            "Claude tool request: {} tools, forced={}", 
            cached_tools.len(), 
            body.get("tool_choice").is_some()
        );
        
        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "context-management-2025-06-27")  // Context management
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("Claude API error {}: {}", status, error_text));
        }
        
        let raw_response: Value = response.json().await?;
        
        // ADDED: Log cache + context management performance
        if let Some(usage) = raw_response["usage"].as_object() {
            let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let cache_write = usage.get("cache_creation_input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let input_tokens = usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            
            if cache_read > 0 {
                let cache_percent = if input_tokens > 0 {
                    (cache_read as f64 / input_tokens as f64) * 100.0
                } else {
                    0.0
                };
                info!("🎯 TOOL CACHE HIT: {} tokens read from cache ({:.1}% of input)", 
                      cache_read, cache_percent);
            }
            
            if cache_write > 0 {
                info!("💾 TOOL CACHE WRITE: {} tokens written to cache", cache_write);
            }
            
            // Log context management if applied
            if let Some(ctx_mgmt) = raw_response.get("context_management") {
                if let Some(original) = ctx_mgmt["original_input_tokens"].as_i64() {
                    let saved = original - input_tokens;
                    info!("✂️  CONTEXT TRIMMED: {} tokens removed ({} -> {})", 
                          saved, original, input_tokens);
                }
            }
        }
        
        // Return raw response (already in Claude format)
        Ok(raw_response)
    }
}
