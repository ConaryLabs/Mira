// src/llm/provider/gpt5.rs
// GPT-5 Responses API implementation with json_schema

use super::{LlmProvider, Message, Response, ToolResponse, TokenUsage, FunctionCall, ToolContext};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::any::Any;
use std::time::Instant;
use tracing::{debug, error};

// Streaming imports
use futures::stream::{Stream, StreamExt};
use tokio_stream::wrappers::LinesStream;
use tokio::io::{AsyncBufReadExt, BufReader};
use std::pin::Pin;
use crate::llm::provider::stream::StreamEvent;
use crate::config::CONFIG;

pub struct Gpt5Provider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: usize,
    verbosity: String,
    reasoning: String,
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
    
    fn flatten_tool_schema(&self, tool: &Value) -> Value {
        if tool["type"] == "function" {
            if let Some(function) = tool.get("function") {
                let mut out = function.clone();
                if let Some(obj) = out.as_object_mut() {
                    obj.insert("type".to_string(), json!("function"));
                }
                return out;
            }
        }
        tool.clone()
    }
    
    /// Build request with json_schema for structured output
    fn build_request(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        context: Option<ToolContext>,
        reasoning: Option<&str>,
        verbosity: Option<&str>,
    ) -> Value {
        let reasoning = reasoning.unwrap_or(&self.reasoning);
        let verbosity = verbosity.unwrap_or(&self.verbosity);
        
        let mut body = json!({
            "model": self.model,
            "stream": true,
            "input": self.format_input(messages, system),
            "max_output_tokens": self.max_tokens,
            "reasoning": {
                "effort": reasoning
            },
            "text": {
                "verbosity": verbosity,
                "format": {
                    "type": "json_schema",
                    "name": "structured_response",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "output": {
                                "type": "string",
                                "description": "Your actual response to the user"
                            },
                            "analysis": {
                                "type": "object",
                                "properties": {
                                    "salience": {
                                        "type": "number",
                                        "description": "Importance score 0.0-1.0"
                                    },
                                    "topics": {
                                        "type": "array",
                                        "items": {"type": "string"},
                                        "description": "Relevant topics from this exchange"
                                    },
                                    "contains_code": {
                                        "type": "boolean",
                                        "description": "Whether response contains code"
                                    },
                                    "programming_lang": {
                                        "type": ["string", "null"],
                                        "description": "Programming language if code present"
                                    },
                                    "contains_error": {
                                        "type": "boolean",
                                        "description": "Whether discussing an error"
                                    },
                                    "error_type": {
                                        "type": ["string", "null"],
                                        "description": "Type of error if present"
                                    },
                                    "routed_to_heads": {
                                        "type": "array",
                                        "items": {"type": "string"},
                                        "description": "Memory heads for routing"
                                    },
                                    "language": {
                                        "type": "string",
                                        "description": "ISO language code"
                                    }
                                },
                                "required": [
                                    "salience",
                                    "topics",
                                    "contains_code",
                                    "programming_lang",
                                    "contains_error",
                                    "error_type",
                                    "routed_to_heads",
                                    "language"
                                ],
                                "additionalProperties": false
                            }
                        },
                        "required": ["output", "analysis"],
                        "additionalProperties": false
                    }
                }
            }
        });
        
        // Add function tools if present
        if !tools.is_empty() {
            let flattened_tools: Vec<Value> = tools.iter()
                .map(|t| self.flatten_tool_schema(t))
                .collect();
            
            if !flattened_tools.is_empty() {
                body["tools"] = Value::Array(flattened_tools);
                // CRITICAL: Don't set tool_choice when using json_schema format
                // The structured output and tool_choice conflict in GPT-5 API
            }
        }
        
        // Handle multi-turn with previous_response_id
        if let Some(ToolContext::Gpt5 { previous_response_id }) = context {
            body["previous_response_id"] = json!(previous_response_id);
            body["input"] = json!([]);
            debug!("GPT-5 multi-turn: continuing from {}", previous_response_id);
        }
        
        body
    }
    
    /// Extract text from json_schema response
    fn extract_text(&self, response: &Value) -> String {
        if CONFIG.debug_logging {
            debug!("RAW GPT-5 RESPONSE: {}", serde_json::to_string_pretty(response).unwrap_or_else(|_| "Failed to serialize".to_string()));
        }
        
        // Check for text content in output array
        if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                // Look for message type with content
                if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                    if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                        for content in content_array {
                            if content.get("type").and_then(|t| t.as_str()) == Some("text") {
                                if let Some(text) = content.get("text").and_then(|t| t.as_str()) {
                                    debug!("Found JSON text output (length: {})", text.len());
                                    return text.to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
        
        error!("Failed to extract text from GPT-5 response - no text content found");
        String::new()
    }
    
    /// Extract function calls from response
    fn extract_function_calls(&self, response: &Value) -> Vec<FunctionCall> {
        let mut calls = Vec::new();
        
        if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    if let (Some(id), Some(name), Some(args)) = (
                        item.get("id").and_then(|i| i.as_str()),
                        item.get("name").and_then(|n| n.as_str()),
                        item.get("arguments"),
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
    
    /// Extract token usage
    fn extract_tokens(&self, response: &Value) -> TokenUsage {
        let input = response
            .pointer("/usage/input_tokens")
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        
        let output = response
            .pointer("/usage/output_tokens")
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        
        let reasoning = response
            .pointer("/usage/reasoning_tokens")
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        
        if reasoning > 0 {
            let reasoning_percent = (reasoning as f64 / output as f64) * 100.0;
            debug!("GPT-5 reasoning: {} tokens ({:.1}% of output)", reasoning, reasoning_percent);
        }
        
        TokenUsage {
            input,
            output,
            reasoning,
            cached: 0,
        }
    }
    
    /// Non-streaming tool calling (provider-specific method)
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
        
        let body = self.build_request(
            messages,
            system,
            tools,
            context,
            reasoning_override,
            verbosity_override,
        );
        
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
    
    /// Streaming tool calling (provider-specific method)
    pub async fn chat_with_tools_streaming(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        context: Option<ToolContext>,
        reasoning_override: Option<&str>,
        verbosity_override: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let body = self.build_request(
            messages,
            system,
            tools,
            context,
            reasoning_override,
            verbosity_override,
        );
        
        if CONFIG.debug_logging {
            debug!("GPT-5 streaming request: {}", serde_json::to_string_pretty(&body)?);
        }
        
        let response = self.client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            error!("GPT-5 streaming API error {}: {}", status, error_text);
            return Err(anyhow!("GPT-5 streaming API error {}: {}", status, error_text));
        }
        
        let stream = response.bytes_stream();
        let reader = stream.map(|r| {
            r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        });
        let async_read = tokio_util::io::StreamReader::new(reader);
        let buf_reader = BufReader::new(async_read);
        let lines = buf_reader.lines();
        
        // Only log if debug_logging is enabled
        if CONFIG.debug_logging {
            debug!("GPT-5 stream created, starting to read SSE events");
        }
        
        Ok(Box::pin(LinesStream::new(lines).filter_map(move |line| async move {
            match line {
                Ok(line) => {
                    if line.is_empty() || line.starts_with(": ") {
                        return None;
                    }
                    
                    let data = line.strip_prefix("data: ")?;
                    if data == "[DONE]" {
                        if CONFIG.debug_logging {
                            debug!("GPT-5 SSE: Received [DONE] marker");
                        }
                        return Some(Ok(StreamEvent::Done {
                            response_id: String::new(),
                            input_tokens: 0,
                            output_tokens: 0,
                            reasoning_tokens: 0,
                            final_text: None,
                        }));
                    }
                    
                    match serde_json::from_str::<Value>(data) {
                        Ok(json) => parse_gpt5_streaming_event(&json),
                        Err(e) => {
                            error!("Failed to parse GPT-5 SSE: {} - Line: {}", e, data);
                            None
                        }
                    }
                }
                Err(e) => Some(Err(anyhow!("Stream error: {}", e))),
            }
        })))
    }
}

// Normalize verbosity values
fn normalize_verbosity(v: &str) -> String {
    match v.to_lowercase().as_str() {
        "low" | "minimal" | "concise" => "low".to_string(),
        "high" | "detailed" | "verbose" => "high".to_string(),
        _ => "medium".to_string(),
    }
}

// Normalize reasoning values
fn normalize_reasoning(r: &str) -> String {
    match r.to_lowercase().as_str() {
        "minimal" | "low" | "quick" => "low".to_string(),
        "high" | "thorough" | "deep" => "high".to_string(),
        _ => "medium".to_string(),
    }
}

#[async_trait]
impl LlmProvider for Gpt5Provider {
    fn name(&self) -> &'static str {
        "gpt-5"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    async fn chat(&self, messages: Vec<Message>, system: String) -> Result<Response> {
        let start = Instant::now();
        
        let body = self.build_request(
            messages,
            system,
            vec![],
            None,
            None,
            None,
        );
        
        let response = self.client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            error!("GPT-5 API error {}: {}", status, error_text);
            return Err(anyhow!("GPT-5 API error {}: {}", status, error_text));
        }
        
        let response_json: Value = response.json().await?;
        let text = self.extract_text(&response_json);
        let tokens = self.extract_tokens(&response_json);
        let latency = start.elapsed().as_millis() as i64;
        
        Ok(Response {
            content: text,
            model: self.model.clone(),
            tokens,
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
        self.chat_with_tools_internal(messages, system, tools, context, None, None).await
    }
}

fn parse_gpt5_streaming_event(json: &Value) -> Option<Result<StreamEvent>> {
    let event_type = json.get("type")?.as_str()?;
    
    match event_type {
        // Text delta - streaming token by token
        "response.output_text.delta" => {
            // Delta is at root level in json_schema format!
            let delta = json.get("delta")?.as_str()?.to_string();
            Some(Ok(StreamEvent::TextDelta { delta }))
        }
        
        // Reasoning delta
        "response.reasoning.delta" => {
            let delta = json.get("delta")?.as_str()?.to_string();
            Some(Ok(StreamEvent::ReasoningDelta { delta }))
        }
        
        // Complete text with structured output (json_schema format)
        "response.output_text.done" => {
            // Don't send this - we already accumulated it from deltas
            // This would duplicate the text
            debug!("Ignoring response.output_text.done - already have text from deltas");
            None
        }
        
        // Content part done - also contains the full text
        "response.content_part.done" => {
            // Also ignore - redundant with deltas
            debug!("Ignoring response.content_part.done - already have text from deltas");
            None
        }
        
        // Tool call deltas
        "response.function_call_arguments.delta" => {
            let id = json.get("call_id")?.as_str()?.to_string();
            let delta = json.get("delta")?.as_str()?.to_string();
            Some(Ok(StreamEvent::ToolCallArgumentsDelta { id, delta }))
        }
        
        // Tool call complete
        "response.function_call_arguments.done" => {
            let id = json.get("call_id")?.as_str()?.to_string();
            let name = json.get("name")?.as_str()?.to_string();
            let arguments = json.get("arguments")?.clone();
            Some(Ok(StreamEvent::ToolCallComplete {
                id,
                name,
                arguments,
            }))
        }
        
        // Output item done - ignore, we already have the text
        "response.output_item.done" => None,
        
        // Final completion event with usage stats
        "response.completed" | "response.done" => {
            let response_obj = json.get("response").unwrap_or(json);
            
            let response_id = response_obj.get("id")
                .and_then(|id| id.as_str())
                .unwrap_or("")
                .to_string();
            
            let input_tokens = response_obj.pointer("/usage/input_tokens")
                .and_then(|t| t.as_i64())
                .unwrap_or(0);
            
            let output_tokens = response_obj.pointer("/usage/output_tokens")
                .and_then(|t| t.as_i64())
                .unwrap_or(0);
            
            let reasoning_tokens = response_obj.pointer("/usage/output_tokens_details/reasoning_tokens")
                .or_else(|| response_obj.pointer("/usage/reasoning_tokens"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0);
            
            debug!("Stream complete: {} input, {} output, {} reasoning tokens", 
                   input_tokens, output_tokens, reasoning_tokens);
            
            Some(Ok(StreamEvent::Done {
                response_id,
                input_tokens,
                output_tokens,
                reasoning_tokens,
                final_text: None,
            }))
        }
        
        _ => {
            debug!("Ignoring unrecognized GPT-5 event type: {}", event_type);
            None
        }
    }
}
