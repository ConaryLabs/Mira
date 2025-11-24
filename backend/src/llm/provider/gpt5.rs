// backend/src/llm/provider/gpt5.rs

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use std::time::Instant;
use tracing::{debug, info};

use super::{LlmProvider, Message, Response, TokenUsage, ToolContext, ToolResponse, FunctionCall};

/// Reasoning effort level for GPT 5.1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningEffort {
    Minimum,
    Medium,
    High,
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReasoningEffort::Minimum => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }
}

impl Default for ReasoningEffort {
    fn default() -> Self {
        ReasoningEffort::Medium
    }
}

/// GPT 5.1 provider using OpenAI API
pub struct Gpt5Provider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    default_reasoning_effort: ReasoningEffort,
}

impl Gpt5Provider {
    /// Create a new GPT 5.1 provider
    pub fn new(
        api_key: String,
        model: String,
        default_reasoning_effort: ReasoningEffort,
    ) -> Result<Self> {
        if api_key.is_empty() {
            return Err(anyhow!("OpenAI API key is required"));
        }

        Ok(Gpt5Provider {
            client: Client::new(),
            api_key,
            base_url: "https://api.openai.com/v1".to_string(),
            model,
            default_reasoning_effort,
        })
    }

    /// Validate the API key by making a minimal API call
    pub async fn validate_api_key(&self) -> Result<()> {
        debug!("Validating OpenAI API key with minimal request");

        let test_messages = vec![Message::user("test".to_string())];
        let request_body = serde_json::json!({
            "model": self.model,
            "messages": test_messages,
            "max_tokens": 1,
        });

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
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
                401 => "Invalid API key. Please check your OpenAI API key.".to_string(),
                403 => "API key does not have permission to access this resource.".to_string(),
                429 => "Rate limit exceeded. Please try again later.".to_string(),
                _ => format!("API validation failed ({}): {}", status, error_text),
            };

            return Err(anyhow!(error_msg));
        }

        info!("API key validation successful");
        Ok(())
    }

    /// Send a completion request with custom reasoning effort
    pub async fn complete_with_reasoning(
        &self,
        messages: Vec<Message>,
        system: String,
        reasoning_effort: ReasoningEffort,
    ) -> Result<Response> {
        let start = Instant::now();
        debug!(
            "Sending request to GPT 5.1 with {} messages, reasoning: {:?}",
            messages.len(),
            reasoning_effort
        );

        // Build messages array (system first, then conversation)
        let mut api_messages = vec![Message::system(system)];
        api_messages.extend(messages);

        let request_body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "reasoning_effort": reasoning_effort.as_str(),
        });

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
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
            return Err(anyhow!("API returned {}: {}", status, error_text));
        }

        let response_body: Value = response.json().await?;
        let latency_ms = start.elapsed().as_millis() as i64;

        self.parse_response(response_body, latency_ms)
    }

    /// Send a completion request with tools and custom reasoning effort
    pub async fn complete_with_tools_and_reasoning(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        reasoning_effort: ReasoningEffort,
    ) -> Result<ToolResponse> {
        let start = Instant::now();
        debug!(
            "Sending tool request to GPT 5.1 with {} messages, {} tools, reasoning: {:?}",
            messages.len(),
            tools.len(),
            reasoning_effort
        );

        // Build messages array
        let mut api_messages = vec![Message::system(system)];
        api_messages.extend(messages);

        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "tools": tools,
            "tool_choice": "auto",
            "reasoning_effort": reasoning_effort.as_str(),
        });

        // Remove empty tools array if no tools
        if tools.is_empty() {
            request_body.as_object_mut().unwrap().remove("tools");
            request_body.as_object_mut().unwrap().remove("tool_choice");
        }

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
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
            return Err(anyhow!("API returned {}: {}", status, error_text));
        }

        let response_body: Value = response.json().await?;
        let latency_ms = start.elapsed().as_millis() as i64;

        self.parse_tool_response(response_body, latency_ms)
    }

    /// Parse regular chat response
    fn parse_response(&self, response: Value, latency_ms: i64) -> Result<Response> {
        let choice = response["choices"][0].clone();
        let message = choice["message"].clone();
        let usage = response["usage"].clone();

        let content = message["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let tokens = TokenUsage {
            input: usage["prompt_tokens"].as_i64().unwrap_or(0),
            output: usage["completion_tokens"].as_i64().unwrap_or(0),
            reasoning: 0, // GPT 5.1 doesn't separate reasoning tokens in standard response
            cached: 0,
        };

        info!(
            "GPT 5.1 response: {} input tokens, {} output tokens",
            tokens.input, tokens.output
        );

        Ok(Response {
            content,
            model: self.model.clone(),
            tokens,
            latency_ms,
        })
    }

    /// Parse tool calling response
    fn parse_tool_response(&self, response: Value, latency_ms: i64) -> Result<ToolResponse> {
        let choice = response["choices"][0].clone();
        let message = choice["message"].clone();
        let usage = response["usage"].clone();

        let text_output = message["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Parse tool calls
        let function_calls: Vec<FunctionCall> = if let Some(calls) = message["tool_calls"].as_array() {
            calls
                .iter()
                .filter_map(|call| {
                    let id = call["id"].as_str()?.to_string();
                    let name = call["function"]["name"].as_str()?.to_string();
                    let arguments: Value = serde_json::from_str(
                        call["function"]["arguments"].as_str()?
                    ).ok()?;

                    Some(FunctionCall {
                        id,
                        name,
                        arguments,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let tokens = TokenUsage {
            input: usage["prompt_tokens"].as_i64().unwrap_or(0),
            output: usage["completion_tokens"].as_i64().unwrap_or(0),
            reasoning: 0,
            cached: 0,
        };

        info!(
            "GPT 5.1 tool response: {} input tokens, {} output tokens, {} tool calls",
            tokens.input,
            tokens.output,
            function_calls.len()
        );

        Ok(ToolResponse {
            id: choice["id"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            text_output,
            function_calls,
            tokens,
            latency_ms,
            raw_response: response,
        })
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
        self.complete_with_reasoning(messages, system, self.default_reasoning_effort)
            .await
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        _context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        self.complete_with_tools_and_reasoning(
            messages,
            system,
            tools,
            self.default_reasoning_effort,
        )
        .await
    }

    async fn stream(
        &self,
        messages: Vec<Message>,
        system: String,
    ) -> Result<Box<dyn futures::Stream<Item = Result<String>> + Send + Unpin>> {
        debug!("Sending streaming request to GPT 5.1 with {} messages", messages.len());

        // Build messages array
        let mut api_messages = vec![Message::system(system)];
        api_messages.extend(messages);

        let request_body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "stream": true,
            "reasoning_effort": self.default_reasoning_effort.as_str(),
        });

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
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
            return Err(anyhow!("API returned {}: {}", status, error_text));
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
                                if data == "[DONE]" {
                                    return Some(Ok(String::new()));
                                }

                                // Parse JSON chunk
                                if let Ok(json) = serde_json::from_str::<Value>(data) {
                                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                                        return Some(Ok(content.to_string()));
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
