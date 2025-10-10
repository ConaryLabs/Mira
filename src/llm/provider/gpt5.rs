// src/llm/provider/gpt5.rs
// GPT-5 Responses API implementation

use super::{LlmProvider, Message, Response, ToolResponse, TokenUsage, FunctionCall, ToolContext};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::any::Any;
use std::time::Instant;
use tracing::{debug, error, info};

// Streaming imports
use futures::stream::{Stream, StreamExt};
use tokio_stream::wrappers::LinesStream;
use tokio::io::{AsyncBufReadExt, BufReader};
use std::pin::Pin;
use crate::llm::provider::stream::StreamEvent;

// Use global config for logging controls
use crate::config::CONFIG;

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
                let _flattened = json!({
                    "type": "function"
                });
                
                // Copy all fields from the nested "function" object
                if let Some(_obj) = function.as_object() {
                    for (_key, _value) in _obj {
                        // NOTE: serde_json::Value doesn't provide direct index assignment on a temporary,
                        // so we build a mutable object map when needed.
                    }
                }
                
                // A safer flatten: clone and then set type
                let mut out = function.clone();
                if let Some(obj) = out.as_object_mut() {
                    obj.insert("type".to_string(), json!("function"));
                }
                return out;
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
        // Dump the entire response (gated by debug flag)
        if CONFIG.debug_logging {
            error!("RAW GPT-5 RESPONSE: {}", serde_json::to_string_pretty(response).unwrap_or_else(|_| "Failed to serialize".to_string()));
        } else {
            debug!("RAW GPT-5 RESPONSE (truncated log)");
        }
        
        // FIRST: Check for structured JSON output (when using json_schema)
        if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
            error!("Found output array with {} items", output_array.len());
            
            for (idx, item) in output_array.iter().enumerate() {
                error!("  Item {}: type = {:?}", idx, item.get("type"));
                
                // Type "message" with JSON content
                if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                    error!("  Found message type at index {}", idx);
                    
                    if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                        error!("    Content array has {} items", content_array.len());
                        
                        for (cidx, content) in content_array.iter().enumerate() {
                            debug!("      Content {}: type = {:?}", cidx, content.get("type"));
                            
                            // Look for output_text type (structured JSON)
                            if content.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                if let Some(text) = content.get("text").and_then(|t| t.as_str()) {
                                    debug!("Found structured JSON in output_text (length: {})", text.len());
                                    return text.to_string();
                                } else {
                                    debug!("output_text exists but no text field");
                                }
                            }
                            
                            // Also try regular "text" type
                            if content.get("type").and_then(|t| t.as_str()) == Some("text") {
                                if let Some(text) = content.get("text").and_then(|t| t.as_str()) {
                                    debug!("Found text in regular text field (length: {})", text.len());
                                    return text.to_string();
                                }
                            }
                        }
                    } else {
                        debug!("    No content array in message");
                    }
                }
            }
            debug!("No structured JSON found in output array");
        } else {
            debug!("No output array in response");
        }
        
        // FALLBACK 1: Direct path /output/1/content/0/text
        if let Some(text) = response.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
            debug!("Found text via JSON pointer /output/1/content/0/text");
            return text.to_string();
        }
        
        // FALLBACK 2: Convenience field output_text
        if let Some(text) = response.get("output_text").and_then(|t| t.as_str()) {
            debug!("Found text via output_text convenience field");
            return text.to_string();
        }
        
        // FALLBACK 3: Try output.message.content[0].text
        if let Some(text) = response.pointer("/output/message/content/0/text").and_then(|t| t.as_str()) {
            debug!("Found text via /output/message/content/0/text");
            return text.to_string();
        }
        
        error!("ALL TEXT EXTRACTION PATHS FAILED - returning empty string");
        "".to_string()
    }
    
    /// Helper: parse arguments which may be a JSON object or a stringified JSON
    fn parse_arguments_value(args_val: &Value) -> Option<Value> {
        if args_val.is_object() {
            return Some(args_val.clone());
        }
        if let Some(s) = args_val.as_str() {
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                return Some(parsed);
            }
        }
        None
    }

    /// Extract function calls from Responses API (robust to schema variants)
    fn extract_function_calls(&self, response: &Value) -> Vec<FunctionCall> {
        let mut calls = Vec::new();
        
        if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                let item_type = item.get("type").and_then(|t| t.as_str());
                match item_type {
                    // Top-level function/tool call variants
                    Some("function_call") | Some("tool_call") => {
                        let id = item.get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(|v| v.as_str());
                        let name = item.get("name").and_then(|v| v.as_str());
                        let args_val = item.get("arguments");

                        if let (Some(id), Some(name), Some(args_val)) = (id, name, args_val) {
                            if let Some(arguments) = Self::parse_arguments_value(args_val) {
                                calls.push(FunctionCall {
                                    id: id.to_string(),
                                    name: name.to_string(),
                                    arguments,
                                });
                            }
                        }
                    }
                    // Message content may contain tool uses
                    Some("message") => {
                        if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                            for content in content_array {
                                let ctype = content.get("type").and_then(|t| t.as_str());
                                if matches!(ctype, Some("tool_use") | Some("function_call")) {
                                    let id = content.get("id")
                                        .or_else(|| content.get("call_id"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let name = content.get("name").and_then(|v| v.as_str());
                                    let args_val = content.get("arguments").or_else(|| content.get("input"));
                                    if let (Some(name), Some(args_val)) = (name, args_val) {
                                        if let Some(arguments) = Self::parse_arguments_value(args_val) {
                                            calls.push(FunctionCall {
                                                id: id.to_string(),
                                                name: name.to_string(),
                                                arguments,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
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
        
        // Extract reasoning tokens from output_tokens_details
        let reasoning = usage["output_tokens_details"]["reasoning_tokens"].as_i64().unwrap_or(0);
        
        // Log reasoning token usage
        if reasoning > 0 {
            let reasoning_percent = if output > 0 {
                (reasoning as f64 / output as f64) * 100.0
            } else {
                0.0
            };
            info!("GPT-5 reasoning: {} tokens ({:.1}% of output)", reasoning, reasoning_percent);
        }
        
        TokenUsage {
            input,
            output,
            reasoning,
            cached: 0,  // GPT-5 doesn't expose cache metrics
        }
    }
    
    /// Internal method with reasoning/verbosity overrides for orchestrator
    /// CRITICAL: Use this for dynamic reasoning/verbosity per iteration
    pub async fn chat_with_tools_internal(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        context: Option<ToolContext>,
        reasoning_override: Option<&str>,
        verbosity_override: Option<&str>,
    ) -> Result<ToolResponse> {
        let start = Instant::now();
        
        // Use overrides if provided, otherwise fall back to instance defaults
        let reasoning = reasoning_override.unwrap_or(&self.reasoning);
        let verbosity = verbosity_override.unwrap_or(&self.verbosity);
        
        let mut body = json!({
            "model": self.model,
            "input": self.format_input(messages.clone(), system.clone()),
            "instructions": system,  // CRITICAL: Must provide every time
            "max_output_tokens": self.max_tokens,
            "tools": tools.iter().map(|t| self.flatten_tool_schema(t)).collect::<Vec<_>>(),
            "tool_choice": {"type": "auto"},
            "reasoning": {
                "effort": reasoning
            },
            "text": {
                "verbosity": verbosity,
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
        });
        
        // CRITICAL: Handle previous_response_id for multi-turn
        if let Some(ToolContext::Gpt5 { previous_response_id }) = context {
            body["previous_response_id"] = json!(previous_response_id);
            // When using previous_response_id, send empty input to save tokens
            body["input"] = json!([]);
            debug!("GPT-5 multi-turn: continuing from {}", previous_response_id);
        }
        
        debug!("GPT-5 tool request: reasoning={}, verbosity={}", reasoning, verbosity);
        
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
        
        // Extract response_id - we need this for next iteration!
        let response_id = raw["id"].as_str().unwrap_or("").to_string();
        
        Ok(ToolResponse {
            id: response_id,
            text_output: self.extract_text(&raw),
            function_calls: self.extract_function_calls(&raw),
            tokens: self.extract_tokens(&raw),
            latency_ms: latency,
            raw_response: raw,
        })
    }
    
    /// Streaming version of chat_with_tools_internal
    pub async fn chat_with_tools_streaming(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        context: Option<ToolContext>,
        reasoning_override: Option<&str>,
        verbosity_override: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let reasoning = reasoning_override.unwrap_or(&self.reasoning);
        let verbosity = verbosity_override.unwrap_or(&self.verbosity);
        
        let mut body = json!({
            "model": self.model,
            "input": self.format_input(messages.clone(), system.clone()),
            "instructions": system,
            "max_output_tokens": self.max_tokens,
            "tools": tools.iter().map(|t| self.flatten_tool_schema(t)).collect::<Vec<_>>(),
            "tool_choice": {"type": "auto"},
            "reasoning": {
                "effort": reasoning
            },
            "text": {
                "verbosity": verbosity,
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
            "stream": true,  // ENABLE STREAMING
        });
        
        // Handle previous_response_id
        if let Some(ToolContext::Gpt5 { previous_response_id }) = context {
            body["previous_response_id"] = json!(previous_response_id);
            body["input"] = json!([]);
            debug!("GPT-5 streaming: continuing from {}", previous_response_id);
        }
        
        debug!("GPT-5 streaming request: reasoning={}, verbosity={}", reasoning, verbosity);
        
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
        
        // Convert response bytes to line stream
        let byte_stream = response.bytes_stream();
        let reader = tokio_util::io::StreamReader::new(
            byte_stream.map(|result| result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)))
        );
        let buf_reader = BufReader::new(reader);
        let lines = buf_reader.lines();
        let line_stream = LinesStream::new(lines);
        
        // Convert lines to StreamEvents
        let event_stream = line_stream.filter_map(|line_result| async move {
            match line_result {
                Ok(line) => {
                    if line.is_empty() {
                        // Empty lines separate events in SSE
                        return None;
                    }
                    
                    // Parse the SSE line into a StreamEvent
                    StreamEvent::from_sse_line(&line).map(Ok)
                }
                Err(e) => {
                    Some(Err(anyhow!("Stream read error: {}", e)))
                }
            }
        });
        
        Ok(Box::pin(event_stream))
    }
}

#[async_trait]
impl LlmProvider for Gpt5Provider {
    fn name(&self) -> &'static str {
        "gpt5"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
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
        // Just calls internal with defaults - orchestrator uses internal method directly
        self.chat_with_tools_internal(messages, system, tools, context, None, None).await
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
