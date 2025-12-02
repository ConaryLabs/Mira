// backend/src/llm/provider/gemini3.rs
// Gemini 3 Pro provider using Google AI API

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use std::time::Instant;
use tracing::{debug, info};

use super::{FunctionCall, LlmProvider, Message, Response, TokenUsage, ToolContext, ToolResponse};
use crate::prompt::internal::llm as prompts;

// ============================================================================
// Thinking Level (replaces ThinkingLevel)
// ============================================================================

/// Thinking level for Gemini 3 (only Low and High available)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingLevel {
    Low,
    High,
}

impl ThinkingLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThinkingLevel::Low => "low",
            ThinkingLevel::High => "high",
        }
    }
}

impl Default for ThinkingLevel {
    fn default() -> Self {
        ThinkingLevel::High
    }
}

// ============================================================================
// Pricing
// ============================================================================

/// Gemini 3 Pro Preview pricing (per 1M tokens)
/// Source: https://ai.google.dev/gemini-api/docs/pricing
/// Model: gemini-3-pro-preview (released Nov 2025)
pub struct Gemini3Pricing;

impl Gemini3Pricing {
    /// Input token price per 1M tokens (USD) - under 200k context
    const INPUT_PRICE_PER_M: f64 = 2.00;
    /// Input token price per 1M tokens (USD) - over 200k context
    const INPUT_PRICE_PER_M_LARGE: f64 = 4.00;
    /// Output token price per 1M tokens (USD) - under 200k context
    const OUTPUT_PRICE_PER_M: f64 = 12.00;
    /// Output token price per 1M tokens (USD) - over 200k context
    const OUTPUT_PRICE_PER_M_LARGE: f64 = 18.00;

    /// Calculate cost from token usage (standard context)
    pub fn calculate_cost(tokens_input: i64, tokens_output: i64) -> f64 {
        let input_cost = (tokens_input as f64 / 1_000_000.0) * Self::INPUT_PRICE_PER_M;
        let output_cost = (tokens_output as f64 / 1_000_000.0) * Self::OUTPUT_PRICE_PER_M;
        input_cost + output_cost
    }

    /// Calculate cost with large context pricing (over 200k tokens)
    pub fn calculate_cost_large_context(tokens_input: i64, tokens_output: i64) -> f64 {
        let input_cost = (tokens_input as f64 / 1_000_000.0) * Self::INPUT_PRICE_PER_M_LARGE;
        let output_cost = (tokens_output as f64 / 1_000_000.0) * Self::OUTPUT_PRICE_PER_M_LARGE;
        input_cost + output_cost
    }

    /// Calculate cost with automatic tier selection
    pub fn calculate_cost_auto(tokens_input: i64, tokens_output: i64) -> f64 {
        if tokens_input > 200_000 {
            Self::calculate_cost_large_context(tokens_input, tokens_output)
        } else {
            Self::calculate_cost(tokens_input, tokens_output)
        }
    }
}

// ============================================================================
// Provider
// ============================================================================

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

    /// Convert our Message format to Gemini API format
    fn messages_to_gemini_contents(messages: &[Message], system: &str) -> Vec<Value> {
        let mut contents = Vec::new();

        // Add system instruction as first user message if present
        // Gemini uses systemInstruction separately, but for simplicity we prepend to first user msg
        let system_text = if !system.is_empty() {
            Some(system.to_string())
        } else {
            None
        };

        let mut system_added = false;

        for msg in messages {
            let role = match msg.role.as_str() {
                "user" => "user",
                "assistant" => "model",
                "tool" => "function", // Function response
                "system" => continue, // Skip system messages, handled separately
                _ => "user",
            };

            let mut parts = Vec::new();

            // Add system instruction to first user message
            if role == "user" && !system_added {
                if let Some(ref sys) = system_text {
                    parts.push(serde_json::json!({"text": format!("[System]\n{}\n\n[User]\n", sys)}));
                }
                system_added = true;
            }

            // Handle function responses
            if msg.role == "tool" {
                if let Some(ref call_id) = msg.tool_call_id {
                    contents.push(serde_json::json!({
                        "role": "function",
                        "parts": [{
                            "functionResponse": {
                                "name": call_id,
                                "response": {
                                    "result": msg.content
                                }
                            }
                        }]
                    }));
                    continue;
                }
            }

            // Add text content
            if !msg.content.is_empty() {
                parts.push(serde_json::json!({"text": msg.content}));
            }

            // Add thought signature if present
            if let Some(ref sig) = msg.thought_signature {
                parts.push(serde_json::json!({"thoughtSignature": sig}));
            }

            // Add function calls if present (for model messages)
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    parts.push(serde_json::json!({
                        "functionCall": {
                            "name": tc.name,
                            "args": tc.arguments
                        }
                    }));
                }
            }

            if !parts.is_empty() {
                contents.push(serde_json::json!({
                    "role": role,
                    "parts": parts
                }));
            }
        }

        // If system wasn't added (no user messages), add it as first message
        if !system_added && system_text.is_some() {
            contents.insert(
                0,
                serde_json::json!({
                    "role": "user",
                    "parts": [{"text": system_text.unwrap()}]
                }),
            );
        }

        contents
    }

    /// Convert OpenAI-format tools to Gemini format
    fn tools_to_gemini_format(tools: &[Value]) -> Value {
        let function_declarations: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                // Handle OpenAI format: { type: "function", function: { name, description, parameters } }
                if let Some(func) = tool.get("function") {
                    Some(serde_json::json!({
                        "name": func.get("name"),
                        "description": func.get("description"),
                        "parameters": func.get("parameters")
                    }))
                } else if tool.get("name").is_some() {
                    // Already in simple format
                    Some(tool.clone())
                } else {
                    None
                }
            })
            .collect();

        serde_json::json!({
            "functionDeclarations": function_declarations
        })
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

        let contents = Self::messages_to_gemini_contents(&messages, &system);

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
        let candidate = response
            .get("candidates")
            .and_then(|c| c.get(0))
            .ok_or_else(|| anyhow!("No candidates in Gemini response"))?;

        let content = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        let usage = response.get("usageMetadata");
        let tokens = TokenUsage {
            input: usage
                .and_then(|u| u.get("promptTokenCount"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
            output: usage
                .and_then(|u| u.get("candidatesTokenCount"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
            reasoning: usage
                .and_then(|u| u.get("thoughtsTokenCount"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
            cached: usage
                .and_then(|u| u.get("cachedContentTokenCount"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
        };

        // Log token usage with cache info
        if tokens.cached > 0 {
            let cache_percent = (tokens.cached as f64 / tokens.input as f64 * 100.0) as i64;
            info!(
                "Gemini 3 response: {} input ({} cached = {}% savings), {} output, {} thinking",
                tokens.input, tokens.cached, cache_percent, tokens.output, tokens.reasoning
            );
        } else {
            info!(
                "Gemini 3 response: {} input tokens, {} output tokens, {} thinking tokens (no cache hit)",
                tokens.input, tokens.output, tokens.reasoning
            );
        }

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

        let contents = Self::messages_to_gemini_contents(&messages, &system);
        let gemini_tools = Self::tools_to_gemini_format(&tools);

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
        let candidate = response
            .get("candidates")
            .and_then(|c| c.get(0))
            .ok_or_else(|| anyhow!("No candidates in Gemini response"))?;

        let finish_reason = candidate
            .get("finishReason")
            .and_then(|f| f.as_str())
            .unwrap_or("UNKNOWN")
            .to_string();

        let parts = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array());

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

        // Extract usage
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
        if tokens_cached > 0 {
            let cache_percent = (tokens_cached as f64 / tokens_input as f64 * 100.0) as i64;
            info!(
                "Gemini 3 tool call: {} input ({} cached = {}% savings), {} output",
                tokens_input, tokens_cached, cache_percent, tokens_output
            );
        }

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
    pub async fn generate_code(&self, request: CodeGenRequest) -> Result<CodeGenResponse> {
        info!(
            "Gemini 3: Generating {} code at {}",
            request.language, request.path
        );

        let system_prompt = prompts::code_gen_specialist(&request.language);
        let user_prompt = build_user_prompt(&request);

        debug!("Gemini 3 user prompt:\n{}", user_prompt);

        let request_body = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [{
                    "text": format!("{}\n\n{}", system_prompt, user_prompt)
                }]
            }],
            "generationConfig": {
                "temperature": 1.0,
                "responseMimeType": "application/json"
            }
        });

        let response = self
            .client
            .post(self.api_url("generateContent"))
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
            return Err(anyhow!("Gemini 3 API error {}: {}", status, error_text));
        }

        let response_json: Value = response.json().await?;

        // Extract content from response
        let content_str = response_json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("Invalid Gemini 3 response structure"))?;

        // Parse the JSON content
        let artifact: CodeArtifact = serde_json::from_str(content_str)?;

        // Extract token usage
        let usage = response_json.get("usageMetadata");
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

        if tokens_cached > 0 {
            let cache_percent = (tokens_cached as f64 / tokens_input as f64 * 100.0) as i64;
            info!(
                "Gemini 3: Generated {} lines at {} ({} cached = {}% savings)",
                artifact.content.lines().count(), artifact.path, tokens_cached, cache_percent
            );
        } else {
            info!(
                "Gemini 3: Generated {} lines of code at {}",
                artifact.content.lines().count(),
                artifact.path
            );
        }

        Ok(CodeGenResponse {
            artifact,
            tokens_input,
            tokens_output,
        })
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

        let contents = Self::messages_to_gemini_contents(&messages, &system);
        let gemini_tools = Self::tools_to_gemini_format(&tools);

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
        let candidate = response
            .get("candidates")
            .and_then(|c| c.get(0))
            .ok_or_else(|| anyhow!("No candidates in Gemini response"))?;

        let parts = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array());

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

        let usage = response.get("usageMetadata");
        let tokens = TokenUsage {
            input: usage
                .and_then(|u| u.get("promptTokenCount"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
            output: usage
                .and_then(|u| u.get("candidatesTokenCount"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
            reasoning: usage
                .and_then(|u| u.get("thoughtsTokenCount"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
            cached: usage
                .and_then(|u| u.get("cachedContentTokenCount"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
        };

        if tokens.cached > 0 {
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

        let contents = Self::messages_to_gemini_contents(&messages, &system);

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
    /// Thought signature for multi-turn conversations (MUST be passed back)
    pub thought_signature: Option<String>,
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
