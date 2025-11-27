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
use crate::prompt::internal::llm as prompts;

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

/// GPT 5.1 pricing (per 1M tokens)
/// Source: https://openai.com/api/pricing/
pub struct Gpt5Pricing;

impl Gpt5Pricing {
    /// Input token price per 1M tokens (USD)
    const INPUT_PRICE_PER_M: f64 = 1.25;
    /// Cached input token price per 1M tokens (USD) - 90% discount
    const CACHED_INPUT_PRICE_PER_M: f64 = 0.125;
    /// Output token price per 1M tokens (USD)
    const OUTPUT_PRICE_PER_M: f64 = 10.00;

    /// Calculate cost from token usage (uncached)
    pub fn calculate_cost(tokens_input: i64, tokens_output: i64) -> f64 {
        let input_cost = (tokens_input as f64 / 1_000_000.0) * Self::INPUT_PRICE_PER_M;
        let output_cost = (tokens_output as f64 / 1_000_000.0) * Self::OUTPUT_PRICE_PER_M;
        input_cost + output_cost
    }

    /// Calculate cost with cached input tokens
    pub fn calculate_cost_with_cache(
        tokens_input: i64,
        tokens_cached: i64,
        tokens_output: i64,
    ) -> f64 {
        let uncached_input = tokens_input - tokens_cached;
        let input_cost = (uncached_input as f64 / 1_000_000.0) * Self::INPUT_PRICE_PER_M;
        let cached_cost = (tokens_cached as f64 / 1_000_000.0) * Self::CACHED_INPUT_PRICE_PER_M;
        let output_cost = (tokens_output as f64 / 1_000_000.0) * Self::OUTPUT_PRICE_PER_M;
        input_cost + cached_cost + output_cost
    }
}

impl Default for ReasoningEffort {
    fn default() -> Self {
        ReasoningEffort::Medium
    }
}

/// GPT 5.1 provider using OpenAI API
#[derive(Clone)]
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

    /// Check if provider is configured and available
    pub fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Get the model name
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the default reasoning effort
    pub fn reasoning_effort(&self) -> ReasoningEffort {
        self.default_reasoning_effort
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

    /// Call with tools - matches the interface expected by orchestrators
    pub async fn call_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<Value>,
    ) -> Result<ToolCallResponse> {
        info!(
            "GPT 5.1: Calling with {} tools, {} messages",
            tools.len(),
            messages.len()
        );

        // Convert tools to OpenAI format if needed
        let openai_tools = Self::convert_tools_to_openai_format(&tools);

        // Convert our Message format to API format
        let api_messages: Vec<Value> = messages
            .iter()
            .map(|msg| {
                let mut obj = serde_json::json!({
                    "role": msg.role,
                    "content": msg.content
                });

                // Add tool_call_id for tool response messages
                if let Some(ref call_id) = msg.tool_call_id {
                    obj["tool_call_id"] = Value::String(call_id.clone());
                }

                // Add tool_calls for assistant messages requesting tool execution
                if let Some(ref tool_calls) = msg.tool_calls {
                    obj["tool_calls"] = serde_json::json!(tool_calls.iter().map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default()
                            }
                        })
                    }).collect::<Vec<_>>());
                }

                obj
            })
            .collect();

        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "tools": openai_tools,
            "tool_choice": "auto",
            "reasoning_effort": self.default_reasoning_effort.as_str(),
        });

        // Remove empty tools array if no tools
        if tools.is_empty() {
            request_body.as_object_mut().unwrap().remove("tools");
            request_body.as_object_mut().unwrap().remove("tool_choice");
        }

        debug!(
            "GPT 5.1 tool calling request:\n{}",
            serde_json::to_string_pretty(&request_body).unwrap_or_default()
        );

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
            return Err(anyhow!("GPT 5.1 API error {}: {}", status, error_text));
        }

        let response_json: Value = response.json().await?;

        debug!(
            "GPT 5.1 tool calling response:\n{}",
            serde_json::to_string_pretty(&response_json).unwrap_or_default()
        );

        // Extract usage information
        let usage = response_json.get("usage");
        let tokens_input = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        let tokens_output = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);

        // Extract the choice
        let choice = response_json
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or_else(|| anyhow!("No choices in GPT 5.1 response"))?;

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow!("No message in GPT 5.1 choice"))?;

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Extract content (may be null if tool calls are present)
        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        // Extract tool calls if present
        let mut tool_calls = Vec::new();
        if let Some(calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
            for call in calls {
                let id = call
                    .get("id")
                    .and_then(|i| i.as_str())
                    .ok_or_else(|| anyhow!("Missing tool call id"))?
                    .to_string();

                let function = call
                    .get("function")
                    .ok_or_else(|| anyhow!("Missing function in tool call"))?;

                let name = function
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| anyhow!("Missing function name"))?
                    .to_string();

                let arguments_str = function
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .ok_or_else(|| anyhow!("Missing function arguments"))?;

                let arguments: Value = serde_json::from_str(arguments_str)?;

                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }

        if !tool_calls.is_empty() {
            info!(
                "GPT 5.1 returned {} tool call(s): {:?}",
                tool_calls.len(),
                tool_calls.iter().map(|tc| &tc.name).collect::<Vec<_>>()
            );
        } else if let Some(ref text) = content {
            info!("GPT 5.1 returned text response: {} chars", text.len());
        }

        Ok(ToolCallResponse {
            content,
            tool_calls,
            finish_reason,
            tokens_input,
            tokens_output,
        })
    }

    /// Convert tools to OpenAI-compatible format
    fn convert_tools_to_openai_format(tools: &[Value]) -> Vec<Value> {
        tools
            .iter()
            .map(|tool| {
                // Check if already in OpenAI format (has "function" field)
                if tool.get("function").is_some() {
                    tool.clone()
                } else {
                    // Convert from our internal format to OpenAI format
                    serde_json::json!({
                        "type": "function",
                        "function": tool
                    })
                }
            })
            .collect()
    }

    /// Generate code artifact with structured JSON output
    pub async fn generate_code(&self, request: CodeGenRequest) -> Result<CodeGenResponse> {
        info!(
            "GPT 5.1: Generating {} code at {}",
            request.language, request.path
        );

        let system_prompt = prompts::code_gen_specialist(&request.language);

        let user_prompt = build_user_prompt(&request);

        debug!("GPT 5.1 user prompt:\n{}", user_prompt);

        let request_body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "response_format": {"type": "json_object"},
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

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("GPT 5.1 API error {}: {}", status, error_text));
        }

        let response_json: Value = response.json().await?;

        // Extract content from response
        let content_str = response_json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| anyhow!("Invalid GPT 5.1 response structure"))?;

        // Parse the JSON content
        let artifact: CodeArtifact = serde_json::from_str(content_str)?;

        // Extract token usage
        let usage = response_json.get("usage");
        let tokens_input = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        let tokens_output = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);

        info!(
            "GPT 5.1: Generated {} lines of code at {}",
            artifact.content.lines().count(),
            artifact.path
        );

        Ok(CodeGenResponse {
            artifact,
            tokens_input,
            tokens_output,
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

// ============================================================================
// Supporting Types for Code Generation and Tool Calling
// ============================================================================

/// Response from tool calling API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
    pub tokens_input: i64,
    pub tokens_output: i64,
}

/// Individual tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Request to generate code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenRequest {
    pub path: String,
    pub description: String,
    pub language: String,
    pub framework: Option<String>,
    pub dependencies: Vec<String>,
    pub style_guide: Option<String>,
    pub context: String,
}

/// Response from code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenResponse {
    pub artifact: CodeArtifact,
    pub tokens_input: i64,
    pub tokens_output: i64,
}

/// Code artifact generated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeArtifact {
    pub path: String,
    pub content: String,
    pub language: String,
    #[serde(default)]
    pub explanation: Option<String>,
}

/// Build user prompt from request
pub fn build_user_prompt(request: &CodeGenRequest) -> String {
    let mut prompt = format!(
        "Generate a {} file at path: {}\n\n\
        Description: {}\n\n",
        request.language, request.path, request.description
    );

    if let Some(framework) = &request.framework {
        prompt.push_str(&format!("Framework: {}\n\n", framework));
    }

    if !request.dependencies.is_empty() {
        prompt.push_str(&format!(
            "Dependencies: {}\n\n",
            request.dependencies.join(", ")
        ));
    }

    if let Some(style) = &request.style_guide {
        prompt.push_str(&format!("Style preferences: {}\n\n", style));
    }

    if !request.context.is_empty() {
        prompt.push_str(&format!("Additional context:\n{}\n\n", request.context));
    }

    prompt.push_str("Remember: Output ONLY the JSON object, no other text.");

    prompt
}
