// src/llm/provider/gpt5.rs
// GPT-5 Responses API implementation with Custom Tools

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
    
    /// Build custom tool request with Lark grammar for structured output
    fn build_custom_tool_request(
        &self,
        messages: Vec<Message>,
        system: String,
        reasoning: &str,
        verbosity: &str,
    ) -> Value {
        let lark_grammar = r#"
start: output NEWLINE analysis

output: "OUTPUT:" TEXT
analysis: salience NEWLINE topics NEWLINE code NEWLINE prog_lang NEWLINE error NEWLINE error_type NEWLINE routed NEWLINE lang

salience: "SALIENCE:" NUMBER
topics: "TOPICS:" topic ("," topic)*
code: "CONTAINS_CODE:" BOOL
prog_lang: "PROGRAMMING_LANG:" (TEXT | "null")
error: "CONTAINS_ERROR:" BOOL
error_type: "ERROR_TYPE:" (TEXT | "null")
routed: "ROUTED_TO_HEADS:" head ("," head)*
lang: "LANGUAGE:" TEXT

topic: /[^,\n]+/
head: /[^,\n]+/
TEXT: /[^\n]+/
NUMBER: /\d+\.\d+/
BOOL: "true" | "false"
NEWLINE: /\n/
"#;

        json!({
            "model": self.model,
            "stream": true,
            "input": self.format_input(messages, system.clone()),
            "instructions": system,
            "max_output_tokens": self.max_tokens,
            "reasoning": {
                "effort": reasoning
            },
            "text": {
                "verbosity": verbosity
            },
            "tools": [{
                "type": "custom",
                "name": "structured_response",
                "description": "Return structured response with analysis metadata in exact grammar format",
                "format": {
                    "type": "grammar",
                    "syntax": "lark",
                    "definition": lark_grammar
                }
            }],
            "tool_choice": {
                "type": "tool",
                "name": "structured_response"
            }
        })
    }
    
    /// Build custom tool request with additional function tools
    fn build_custom_tool_request_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        context: Option<ToolContext>,
        reasoning: &str,
        verbosity: &str,
    ) -> Value {
        let mut body = self.build_custom_tool_request(messages, system, reasoning, verbosity);
        
        // Add regular function tools if present (skip respond_to_user)
        if !tools.is_empty() {
            let flattened_tools: Vec<Value> = tools.iter()
                .filter(|t| {
                    if let Some(name) = t.pointer("/function/name").or_else(|| t.get("name")) {
                        name.as_str() != Some("respond_to_user")
                    } else {
                        true
                    }
                })
                .map(|t| self.flatten_tool_schema(t))
                .collect();
            
            if !flattened_tools.is_empty() {
                if let Some(existing_tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) {
                    existing_tools.extend(flattened_tools);
                }
                // Enable auto tool selection when we have multiple tools
                body["tool_choice"] = json!("auto");
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
    
    /// Extract text from custom tool output (plaintext grammar format)
    fn extract_text(&self, response: &Value) -> String {
        if CONFIG.debug_logging {
            error!("RAW GPT-5 RESPONSE: {}", serde_json::to_string_pretty(response).unwrap_or_else(|_| "Failed to serialize".to_string()));
        }
        
        // Check for custom tool output in the response
        if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                // Look for tool_call type
                if item.get("type").and_then(|t| t.as_str()) == Some("tool_call") {
                    if item.get("name").and_then(|n| n.as_str()) == Some("structured_response") {
                        if let Some(args) = item.get("arguments").and_then(|a| a.as_str()) {
                            debug!("Found custom tool output (length: {})", args.len());
                            return args.to_string();
                        }
                    }
                }
                
                // Also check message content for tool_use
                if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                    if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                        for content in content_array {
                            if content.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                if content.get("name").and_then(|n| n.as_str()) == Some("structured_response") {
                                    if let Some(input) = content.get("input").and_then(|i| i.as_str()) {
                                        debug!("Found custom tool in message content");
                                        return input.to_string();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        error!("Failed to extract custom tool output");
        "".to_string()
    }
    
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

    fn extract_function_calls(&self, response: &Value) -> Vec<FunctionCall> {
        let mut calls = Vec::new();
        
        if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                let item_type = item.get("type").and_then(|t| t.as_str());
                match item_type {
                    Some("function_call") | Some("tool_call") => {
                        // Skip the structured_response custom tool
                        if item.get("name").and_then(|n| n.as_str()) == Some("structured_response") {
                            continue;
                        }
                        
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
                    Some("message") => {
                        if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                            for content in content_array {
                                let ctype = content.get("type").and_then(|t| t.as_str());
                                if matches!(ctype, Some("tool_use") | Some("function_call")) {
                                    // Skip structured_response
                                    if content.get("name").and_then(|n| n.as_str()) == Some("structured_response") {
                                        continue;
                                    }
                                    
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
    
    fn extract_tokens(&self, response: &Value) -> TokenUsage {
        let usage = &response["usage"];
        let input = usage["input_tokens"].as_i64().unwrap_or(0);
        let output = usage["output_tokens"].as_i64().unwrap_or(0);
        let reasoning = usage["output_tokens_details"]["reasoning_tokens"].as_i64().unwrap_or(0);
        
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
            cached: 0,
        }
    }
    
    /// Non-streaming tool calling with custom tools
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
        let reasoning = reasoning_override.unwrap_or(&self.reasoning);
        let verbosity = verbosity_override.unwrap_or(&self.verbosity);
        
        let body = self.build_custom_tool_request_with_tools(
            messages,
            system,
            tools,
            context,
            reasoning,
            verbosity,
        );
        
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
    
    /// Streaming tool calling with custom tools
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
        
        let body = self.build_custom_tool_request_with_tools(
            messages,
            system,
            tools,
            context,
            reasoning,
            verbosity,
        );
        
        // CRITICAL DEBUG: Log the exact request being sent to GPT-5
        error!("========== GPT-5 REQUEST BODY ==========");
        error!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "Failed to serialize".to_string()));
        error!("========================================");
        
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
        
        let byte_stream = response.bytes_stream();
        let reader = tokio_util::io::StreamReader::new(
            byte_stream.map(|result| result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)))
        );
        let buf_reader = BufReader::new(reader);
        let lines = buf_reader.lines();
        let line_stream = LinesStream::new(lines);
        
        let event_stream = line_stream.filter_map(|line_result| async move {
            match line_result {
                Ok(line) => {
                    if line.is_empty() {
                        return None;
                    }
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
        
        // Simple chat without tools - use basic request
        let body = json!({
            "model": self.model,
            "input": self.format_input(messages, system.clone()),
            "instructions": system,
            "max_output_tokens": self.max_tokens,
            "reasoning": {
                "effort": self.reasoning
            },
            "text": {
                "verbosity": self.verbosity
            }
        });
        
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
        
        // Extract basic text response
        let content = raw.pointer("/output/1/content/0/text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
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
        context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        self.chat_with_tools_internal(messages, system, tools, context, None, None).await
    }
    
    async fn stream(
        &self,
        _messages: Vec<Message>,
        _system: String,
    ) -> Result<Box<dyn futures::Stream<Item = Result<String>> + Send + Unpin>> {
        Err(anyhow!("Use chat_with_tools_streaming instead"))
    }
}

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
