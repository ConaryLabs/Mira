// src/llm/provider/gemini3/mod.rs
// Gemini 3 Pro provider using Google AI API

mod codegen;
mod conversion;
mod pricing;
mod response;
mod types;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::any::Any;
use std::time::Instant;
use tracing::{debug, info};

use super::{FunctionCall, LlmProvider, Message, Response, ToolContext, ToolResponse};

// Re-export public types
pub use codegen::build_user_prompt;
pub use pricing::Gemini3Pricing;
pub use types::{
    CodeArtifact, CodeGenRequest, CodeGenResponse, ThinkingLevel, ToolCall, ToolCallResponse,
};

// Use helper modules
use codegen::generate_code as codegen_generate;
use conversion::{messages_to_gemini_contents, tools_to_gemini_format};
use response::{
    extract_first_candidate, extract_parts, extract_text_content, extract_token_usage,
    log_token_usage, log_tool_call_tokens,
};

/// Gemini 3 Pro provider using Google AI API
#[derive(Clone)]
pub struct Gemini3Provider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    default_thinking_level: ThinkingLevel,
}

impl Gemini3Provider {
    /// Create a new Gemini 3 provider
    pub fn new(
        api_key: String,
        model: String,
        default_thinking_level: ThinkingLevel,
    ) -> Result<Self> {
        if api_key.is_empty() {
            return Err(anyhow!("Google API key is required"));
        }

        Ok(Gemini3Provider {
            client: Client::new(),
            api_key,
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            model,
            default_thinking_level,
        })
    }

    /// Check if provider is configured and available
    pub fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Get the model name
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the default thinking level
    pub fn thinking_level(&self) -> ThinkingLevel {
        self.default_thinking_level
    }

    /// Build the API URL for a given method
    fn api_url(&self, method: &str) -> String {
        format!(
            "{}/models/{}:{}?key={}",
            self.base_url, self.model, method, self.api_key
        )
    }

    /// Validate the API key by making a minimal API call
    pub async fn validate_api_key(&self) -> Result<()> {
        debug!("Validating Google API key with minimal request");

        let request_body = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [{"text": "test"}]
            }],
            "generationConfig": {
                "maxOutputTokens": 1
            }
        });

        let response = self
            .client
            .post(self.api_url("generateContent"))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            let error_msg = match status.as_u16() {
                400 => format!("Invalid request: {}", error_text),
                401 | 403 => "Invalid API key. Please check your Google API key.".to_string(),
                429 => "Rate limit exceeded. Please try again later.".to_string(),
                _ => format!("API validation failed ({}): {}", status, error_text),
            };

            return Err(anyhow!(error_msg));
        }

        info!("Google API key validation successful");
        Ok(())
    }

    /// Send a completion request with custom thinking level
    pub async fn complete_with_thinking(
        &self,
        messages: Vec<Message>,
        system: String,
        thinking_level: ThinkingLevel,
    ) -> Result<Response> {
        let start = Instant::now();
        debug!(
            "Sending request to Gemini 3 with {} messages, thinking: {:?}",
            messages.len(),
            thinking_level
        );

        let contents = messages_to_gemini_contents(&messages, &system);

        let request_body = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": 1.0
            }
        });

        let response = self
            .client
            .post(self.api_url("generateContent"))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Gemini API returned {}: {}", status, error_text));
        }

        let response_body: Value = response.json().await?;
        let latency_ms = start.elapsed().as_millis() as i64;

        self.parse_response(response_body, latency_ms)
    }

    /// Parse regular chat response
    fn parse_response(&self, response: Value, latency_ms: i64) -> Result<Response> {
        let candidate = extract_first_candidate(&response)?;
        let content = extract_text_content(candidate);
        let tokens = extract_token_usage(&response);

        log_token_usage("Gemini 3 response", &tokens);

        Ok(Response {
            content,
            model: self.model.clone(),
            tokens,
            latency_ms,
        })
    }

    /// Call with tools - matches the interface expected by orchestrators
    pub async fn call_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<Value>,
    ) -> Result<ToolCallResponse> {
        info!(
            "Gemini 3: Calling with {} tools, {} messages",
            tools.len(),
            messages.len()
        );

        // Find system message if any
        let system = messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone())
            .unwrap_or_default();

        let contents = messages_to_gemini_contents(&messages, &system);
        let gemini_tools = tools_to_gemini_format(&tools);

        let mut request_body = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": 1.0
            }
        });

        // Add tools if present
        if !tools.is_empty() {
            request_body["tools"] = serde_json::json!([gemini_tools]);
            request_body["toolConfig"] = serde_json::json!({
                "functionCallingConfig": {
                    "mode": "AUTO"
                }
            });
        }

        debug!(
            "Gemini 3 tool calling request:\n{}",
            serde_json::to_string_pretty(&request_body).unwrap_or_default()
        );

        let response = self
            .client
            .post(self.api_url("generateContent"))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Gemini 3 API error {}: {}", status, error_text));
        }

        let response_json: Value = response.json().await?;

        debug!(
            "Gemini 3 tool calling response:\n{}",
            serde_json::to_string_pretty(&response_json).unwrap_or_default()
        );

        self.parse_tool_call_response(response_json)
    }

    /// Parse tool calling response
    fn parse_tool_call_response(&self, response: Value) -> Result<ToolCallResponse> {
        let candidate = extract_first_candidate(&response)?;

        let finish_reason = candidate
            .get("finishReason")
            .and_then(|f| f.as_str())
            .unwrap_or("UNKNOWN")
            .to_string();

        let parts = extract_parts(candidate);

        let mut content = None;
        let mut tool_calls = Vec::new();
        let mut thought_signature = None;

        if let Some(parts_array) = parts {
            for part in parts_array {
                // Extract text content
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    content = Some(text.to_string());
                }

                // Extract thought signature (CRITICAL for multi-turn)
                if let Some(sig) = part.get("thoughtSignature").and_then(|s| s.as_str()) {
                    thought_signature = Some(sig.to_string());
                }

                // Extract function calls
                if let Some(fc) = part.get("functionCall") {
                    let name = fc
                        .get("name")
                        .and_then(|n| n.as_str())
                        .ok_or_else(|| anyhow!("Missing function name"))?
                        .to_string();

                    let arguments = fc.get("args").cloned().unwrap_or(Value::Object(
                        serde_json::Map::new(),
                    ));

                    // Generate a unique ID for the tool call
                    let id = format!("call_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string());

                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
            }
        }

        // Extract usage using helper
        let usage = response.get("usageMetadata");
        let tokens_input = usage
            .and_then(|u| u.get("promptTokenCount"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        let tokens_output = usage
            .and_then(|u| u.get("candidatesTokenCount"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        let tokens_cached = usage
            .and_then(|u| u.get("cachedContentTokenCount"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);

        // Log cache info
        log_tool_call_tokens("Gemini 3 tool call", tokens_input, tokens_output, tokens_cached);

        if !tool_calls.is_empty() {
            info!(
                "Gemini 3 returned {} tool call(s): {:?}",
                tool_calls.len(),
                tool_calls.iter().map(|tc| &tc.name).collect::<Vec<_>>()
            );
        } else if let Some(ref text) = content {
            info!("Gemini 3 returned text response: {} chars", text.len());
        }

        // Log thought signature for debugging
        if thought_signature.is_some() {
            debug!("Gemini 3 returned thought signature (must be passed back in next turn)");
        }

        Ok(ToolCallResponse {
            content,
            tool_calls,
            finish_reason,
            tokens_input,
            tokens_output,
            thought_signature,
        })
    }

    /// Generate code artifact with structured JSON output
    pub async fn generate_code(
        &self,
        request: types::CodeGenRequest,
    ) -> Result<types::CodeGenResponse> {
        codegen_generate(&self.client, &self.api_url("generateContent"), request).await
    }

    /// Send a completion request with tools and custom thinking level
    pub async fn complete_with_tools_and_thinking(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        thinking_level: ThinkingLevel,
    ) -> Result<ToolResponse> {
        let start = Instant::now();
        debug!(
            "Sending tool request to Gemini 3 with {} messages, {} tools, thinking: {:?}",
            messages.len(),
            tools.len(),
            thinking_level
        );

        let contents = messages_to_gemini_contents(&messages, &system);
        let gemini_tools = tools_to_gemini_format(&tools);

        let mut request_body = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": 1.0
            }
        });

        if !tools.is_empty() {
            request_body["tools"] = serde_json::json!([gemini_tools]);
            request_body["toolConfig"] = serde_json::json!({
                "functionCallingConfig": {
                    "mode": "AUTO"
                }
            });
        }

        let response = self
            .client
            .post(self.api_url("generateContent"))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Gemini API returned {}: {}", status, error_text));
        }

        let response_body: Value = response.json().await?;
        let latency_ms = start.elapsed().as_millis() as i64;

        self.parse_tool_response(response_body, latency_ms)
    }

    /// Parse tool response for LlmProvider trait
    fn parse_tool_response(&self, response: Value, latency_ms: i64) -> Result<ToolResponse> {
        let candidate = extract_first_candidate(&response)?;
        let parts = extract_parts(candidate);

        let mut text_output = String::new();
        let mut function_calls = Vec::new();

        if let Some(parts_array) = parts {
            for part in parts_array {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    text_output = text.to_string();
                }

                if let Some(fc) = part.get("functionCall") {
                    let name = fc
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = fc.get("args").cloned().unwrap_or(Value::Null);
                    let id = format!("call_{}", uuid::Uuid::new_v4());

                    function_calls.push(FunctionCall {
                        id,
                        name,
                        arguments,
                    });
                }
            }
        }

        let tokens = extract_token_usage(&response);

        if tokens.cached > 0 && tokens.input > 0 {
            let cache_percent = (tokens.cached as f64 / tokens.input as f64 * 100.0) as i64;
            info!(
                "Gemini 3 tool response: {} input ({} cached = {}% savings), {} output, {} function calls",
                tokens.input, tokens.cached, cache_percent, tokens.output, function_calls.len()
            );
        } else {
            info!(
                "Gemini 3 tool response: {} input tokens, {} output tokens, {} function calls",
                tokens.input, tokens.output, function_calls.len()
            );
        }

        Ok(ToolResponse {
            id: candidate
                .get("index")
                .and_then(|i| i.as_i64())
                .map(|i| i.to_string())
                .unwrap_or_else(|| "0".to_string()),
            text_output,
            function_calls,
            tokens,
            latency_ms,
            raw_response: response,
        })
    }
}

#[async_trait]
impl LlmProvider for Gemini3Provider {
    fn name(&self) -> &'static str {
        "gemini3"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn chat(&self, messages: Vec<Message>, system: String) -> Result<Response> {
        self.complete_with_thinking(messages, system, self.default_thinking_level)
            .await
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        _context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        self.complete_with_tools_and_thinking(messages, system, tools, self.default_thinking_level)
            .await
    }

    async fn stream(
        &self,
        messages: Vec<Message>,
        system: String,
    ) -> Result<Box<dyn futures::Stream<Item = Result<String>> + Send + Unpin>> {
        debug!(
            "Sending streaming request to Gemini 3 with {} messages",
            messages.len()
        );

        let contents = messages_to_gemini_contents(&messages, &system);

        let request_body = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": 1.0
            }
        });

        // Use streamGenerateContent endpoint
        let url = format!(
            "{}/models/{}:streamGenerateContent?key={}&alt=sse",
            self.base_url, self.model, self.api_key
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Gemini API returned {}: {}", status, error_text));
        }

        // Create async stream from SSE response
        let byte_stream = response.bytes_stream();
        let text_stream = byte_stream.filter_map(|chunk_result| async move {
            match chunk_result {
                Ok(bytes) => {
                    if let Ok(text) = std::str::from_utf8(&bytes) {
                        // Parse SSE format
                        for line in text.lines() {
                            let line = line.trim();
                            if line.is_empty() || line.starts_with(':') {
                                continue;
                            }

                            if let Some(data) = line.strip_prefix("data: ") {
                                // Parse JSON chunk
                                if let Ok(json) = serde_json::from_str::<Value>(data) {
                                    if let Some(text) = json
                                        .get("candidates")
                                        .and_then(|c| c.get(0))
                                        .and_then(|c| c.get("content"))
                                        .and_then(|c| c.get("parts"))
                                        .and_then(|p| p.get(0))
                                        .and_then(|p| p.get("text"))
                                        .and_then(|t| t.as_str())
                                    {
                                        return Some(Ok(text.to_string()));
                                    }
                                }
                            }
                        }
                    }
                    None
                }
                Err(e) => Some(Err(anyhow!("Stream error: {}", e))),
            }
        });

        Ok(Box::new(Box::pin(text_stream)))
    }
}
