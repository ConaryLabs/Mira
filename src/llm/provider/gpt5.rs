// src/llm/provider/gpt5.rs
// GPT-5 Responses API implementation

use super::{LlmProvider, Message, Response, ToolResponse, TokenUsage, FunctionCall, ToolContext};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Instant;
use tracing::{debug, info};

pub struct Gpt5Provider {
    client: Client,
    api_key: String,
    model: String,        // "gpt-5"
    max_tokens: usize,    // 128000
    verbosity: String,    // "low" | "medium" | "high"
    reasoning: String,    // "minimal" | "low" | "medium" | "high"
}

impl Gpt5Provider {
    pub fn new(
        api_key: String,
        model: String,
        max_tokens: usize,
        verbosity: String,
        reasoning: String,
    ) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            max_tokens,
            verbosity: normalize_verbosity(&verbosity),
            reasoning: normalize_reasoning(&reasoning),
        }
    }
    
    /// Format messages for GPT-5 Responses API
    fn format_input(&self, messages: Vec<Message>, system: String) -> Vec<Value> {
        let mut input = vec![json!({
            "role": "system",
            "content": system
        })];
        
        for msg in messages {
            input.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }
        
        input
    }
    
    /// Flatten tool schema from OpenAI Chat Completions format to Responses API format
    /// Chat Completions: {"type": "function", "function": {"name": "...", ...}}
    /// Responses API:    {"type": "function", "name": "...", ...}
    fn flatten_tool_schema(&self, tool: &Value) -> Value {
        if tool["type"] == "function" {
            if let Some(function) = tool.get("function") {
                // Flatten: pull everything from "function" up to the top level
                let mut flattened = json!({
                    "type": "function"
                });
                
                // Copy all fields from the nested "function" object
                if let Some(obj) = function.as_object() {
                    for (key, value) in obj {
                        flattened[key] = value.clone();
                    }
                }
                
                return flattened;
            }
        }
        
        // Return as-is if not a nested function tool (e.g., custom tools)
        tool.clone()
    }
    
    /// Build Responses API request body with structured output
    fn build_request(&self, messages: Vec<Message>, system: String, tools: Option<Vec<Value>>) -> Value {
        let mut body = json!({
            "model": self.model,
            "input": self.format_input(messages, system),
            "text": {
                "verbosity": self.verbosity,
                "format": {
                    "type": "json_schema",
                    "name": "response_schema",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "output": {
                                "type": "string",
                                "description": "Your response to the user"
                            },
                            "analysis": {
                                "type": "object",
                                "properties": {
                                    "salience": {"type": "number"},
                                    "topics": {"type": "array", "items": {"type": "string"}},
                                    "contains_code": {"type": "boolean"},
                                    "programming_lang": {"type": ["string", "null"]},
                                    "contains_error": {"type": "boolean"},
                                    "error_type": {"type": ["string", "null"]},
                                    "routed_to_heads": {"type": "array", "items": {"type": "string"}},
                                    "language": {"type": "string"}
                                },
                                "required": ["salience", "topics", "contains_code", "programming_lang", "contains_error", "error_type", "routed_to_heads", "language"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["output", "analysis"],
                        "additionalProperties": false
                    },
                    "strict": true
                }
            },
            "reasoning": {
                "effort": self.reasoning
            },
            "max_output_tokens": self.max_tokens,
        });
        
        if let Some(tools) = tools {
            // Flatten and add tools for file operations, code search, etc.
            let flattened_tools: Vec<Value> = tools.iter()
                .filter(|t| {
                    // Skip respond_to_user - we use structured output instead
                    if let Some(name) = t.pointer("/function/name").or_else(|| t.get("name")) {
                        name.as_str() != Some("respond_to_user")
                    } else {
                        true
                    }
                })
                .map(|t| self.flatten_tool_schema(t))
                .collect();
            
            if !flattened_tools.is_empty() {
                body["tools"] = Value::Array(flattened_tools);
            }
        }
        
        body
    }
    
    /// Extract text from Responses API (multiple fallback paths)
    fn extract_text(&self, response: &Value) -> String {
        // FIRST: Check for structured JSON output (when using json_schema)
        // The output array contains the structured JSON directly
        if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                // Type "message" with JSON content
                if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                    if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                        for content in content_array {
                            // Look for output_text type (structured JSON)
                            if content.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                if let Some(text) = content.get("text").and_then(|t| t.as_str()) {
                                    debug!("Found structured JSON in output_text");
                                    return text.to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // FALLBACK 1: Direct path /output/1/content/0/text
        if let Some(text) = response.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
            return text.to_string();
        }
        
        // FALLBACK 2: Convenience field output_text
        if let Some(text) = response.get("output_text").and_then(|t| t.as_str()) {
            return text.to_string();
        }
        
        // FALLBACK 3: Try output.message.content[0].text
        if let Some(text) = response.pointer("/output/message/content/0/text").and_then(|t| t.as_str()) {
            return text.to_string();
        }
        
        "".to_string()
    }
    
    /// Extract function calls from Responses API
    fn extract_function_calls(&self, response: &Value) -> Vec<FunctionCall> {
        let mut calls = Vec::new();
        
        // Check output array for tool_use items
        if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                if item.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    if let (Some(id), Some(name), Some(args)) = (
                        item.get("id").and_then(|i| i.as_str()),
                        item.get("name").and_then(|n| n.as_str()),
                        item.get("arguments")
                    ) {
                        calls.push(FunctionCall {
                            id: id.to_string(),
                            name: name.to_string(),
                            arguments: args.clone(),
                        });
                    }
                }
            }
        }
        
        calls
    }
    
    /// Extract token usage with reasoning tokens
    fn extract_tokens(&self, response: &Value) -> TokenUsage {
        let usage = &response["usage"];
        let input = usage["input_tokens"].as_i64().unwrap_or(0);
        let output = usage["output_tokens"].as_i64().unwrap_or(0);
        let reasoning = usage["reasoning_tokens"].as_i64().unwrap_or(0);
        
        // Log reasoning token usage
        if reasoning > 0 {
            let reasoning_percent = if output > 0 {
                (reasoning as f64 / (output as f64 + reasoning as f64)) * 100.0
            } else {
                0.0
            };
            info!("🧠 GPT-5 reasoning: {} tokens ({:.1}% of output)", reasoning, reasoning_percent);
        }
        
        TokenUsage {
            input,
            output,
            reasoning,
            cached: 0,  // GPT-5 doesn't expose cache metrics
        }
    }
}

#[async_trait]
impl LlmProvider for Gpt5Provider {
    fn name(&self) -> &'static str {
        "gpt5"
    }
    
    async fn chat(&self, messages: Vec<Message>, system: String) -> Result<Response> {
        let start = Instant::now();
        let body = self.build_request(messages, system, None);
        
        debug!("GPT-5 request: model={}, reasoning.effort={}", self.model, self.reasoning);
        
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
        
        let raw = response.json::<Value>().await?;
        let latency = start.elapsed().as_millis() as i64;
        
        Ok(Response {
            content: self.extract_text(&raw),
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
        context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        let start = Instant::now();
        let mut body = self.build_request(messages.clone(), system.clone(), Some(tools.clone()));
        
        // CRITICAL: Force tool usage - GPT-5 will skip tools if not required
        // This ensures respond_to_user is always called
        body["tool_choice"] = json!("required");
        
        // KEY FEATURE: Use previous_response_id for multi-turn
        // This preserves reasoning context and saves tokens
        if let Some(ToolContext::Gpt5 { previous_response_id }) = context {
            body["previous_response_id"] = json!(previous_response_id);
            debug!("GPT-5 multi-turn: continuing from {}", previous_response_id);
            
            // When using previous_response_id, input is optional
            // Send empty messages to save tokens
            body["input"] = json!([]);
        }
        
        debug!("GPT-5 tool request: {} tools, tool_choice=required", tools.len());
        
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
        
        let raw = response.json::<Value>().await?;
        let latency = start.elapsed().as_millis() as i64;
        
        Ok(ToolResponse {
            id: raw["id"].as_str().unwrap_or("").to_string(),
            text_output: self.extract_text(&raw),
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
        // TODO: Implement streaming for GPT-5 Responses API
        Err(anyhow!("Streaming not yet implemented for GPT-5"))
    }
}

/// Normalize verbosity to valid API values
fn normalize_verbosity(verbosity: &str) -> String {
    match verbosity.to_lowercase().as_str() {
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        _ => {
            debug!("Invalid verbosity '{}', defaulting to 'medium'", verbosity);
            "medium"
        }
    }.to_string()
}

/// Normalize reasoning effort to valid API values
fn normalize_reasoning(effort: &str) -> String {
    match effort.to_lowercase().as_str() {
        "minimal" => "minimal",
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        _ => {
            debug!("Invalid reasoning effort '{}', defaulting to 'medium'", effort);
            "medium"
        }
    }.to_string()
}
