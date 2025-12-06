// src/llm/provider/openai/mod.rs
// OpenAI GPT-5.1 provider implementation using Responses API (December 2025)

pub mod embeddings;
pub mod pricing;
pub mod types;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::any::Any;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use super::{FunctionCall, LlmProvider, Message, Response, TokenUsage, ToolContext, ToolResponse};

// Re-export public types
pub use embeddings::{OpenAIEmbeddingModel, OpenAIEmbeddings};
pub use pricing::{CostResult, OpenAIPricing};
pub use types::{
    OpenAIModel, ReasoningConfig, ReasoningEffort,
    // Native tool types
    NativeToolType, PatchOperation, PatchOpType, ShellResult,
    native_apply_patch_tool, native_shell_tool,
};

// Use types for Responses API
use types::{
    ErrorResponse, InputItem, MessageContent, OutputContent, OutputItem,
    ResponsesInput, ResponsesRequest, ResponsesResponse, ResponsesStreamEvent,
    ResponsesTool,
};

/// OpenAI GPT-5.1 provider using Responses API
#[derive(Clone)]
pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    model: OpenAIModel,
    timeout: Duration,
    /// Reasoning effort for all models
    reasoning_effort: Option<ReasoningEffort>,
}

impl OpenAIProvider {
    /// Base URL for OpenAI API
    const BASE_URL: &'static str = "https://api.openai.com/v1";

    /// Create a new OpenAI provider for GPT-5.1
    pub fn gpt51(api_key: String) -> Result<Self> {
        Self::with_reasoning(api_key, OpenAIModel::Gpt51, ReasoningEffort::Medium)
    }

    /// Create a new OpenAI provider for GPT-5.1 Codex Mini (Fast tier)
    /// Uses "none" reasoning for lowest latency and cost
    pub fn gpt51_mini(api_key: String) -> Result<Self> {
        Self::with_reasoning(api_key, OpenAIModel::Gpt51Mini, ReasoningEffort::None)
    }

    /// Create a new OpenAI provider for GPT-5.1-Codex-Max (Code tier)
    /// Uses "high" reasoning effort for code-focused tasks
    pub fn codex_max(api_key: String) -> Result<Self> {
        Self::with_reasoning(api_key, OpenAIModel::Gpt51CodexMax, ReasoningEffort::High)
    }

    /// Create a new OpenAI provider for GPT-5.1-Codex-Max (Agentic tier)
    /// Uses "xhigh" reasoning effort for long-running autonomous tasks
    pub fn codex_max_agentic(api_key: String) -> Result<Self> {
        Self::with_reasoning(api_key, OpenAIModel::Gpt51CodexMax, ReasoningEffort::XHigh)
    }

    /// Create a new OpenAI provider with specified model
    pub fn new(api_key: String, model: OpenAIModel) -> Result<Self> {
        Self::with_reasoning_opt(api_key, model, None)
    }

    /// Create a provider with specified reasoning effort
    pub fn with_reasoning(api_key: String, model: OpenAIModel, effort: ReasoningEffort) -> Result<Self> {
        Self::with_reasoning_opt(api_key, model, Some(effort))
    }

    /// Internal constructor with optional reasoning
    fn with_reasoning_opt(api_key: String, model: OpenAIModel, reasoning_effort: Option<ReasoningEffort>) -> Result<Self> {
        if api_key.is_empty() {
            return Err(anyhow!("OpenAI API key is required"));
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        Ok(Self {
            client,
            api_key,
            model,
            timeout: Duration::from_secs(120),
            reasoning_effort,
        })
    }

    /// Create provider with custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Get the model being used
    pub fn model(&self) -> OpenAIModel {
        self.model
    }

    /// Check if provider is configured
    pub fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Get reasoning effort (if configured)
    pub fn reasoning_effort(&self) -> Option<ReasoningEffort> {
        self.reasoning_effort
    }

    /// Build reasoning config for API requests
    fn build_reasoning_config(&self) -> Option<ReasoningConfig> {
        self.reasoning_effort.map(|effort| ReasoningConfig { effort })
    }

    /// Convert internal messages to Responses API input format
    fn messages_to_input(&self, messages: &[Message], system: &str) -> ResponsesInput {
        let mut items: Vec<InputItem> = Vec::new();

        // Add messages as input items
        for msg in messages {
            let role = msg.role.clone();
            let content = MessageContent::Text(msg.content.clone());

            items.push(InputItem::Message { role, content });
        }

        // If no messages, use system as input
        if items.is_empty() {
            return ResponsesInput::Text(system.to_string());
        }

        ResponsesInput::Items(items)
    }

    /// Convert tools to Responses API format
    /// Automatically adds native apply_patch and shell tools for code/agentic tiers
    fn tools_to_responses(&self, tools: &[Value]) -> Vec<ResponsesTool> {
        let mut responses_tools: Vec<ResponsesTool> = tools
            .iter()
            .filter_map(|tool| {
                // Extract function declarations from Gemini-style tool format
                if let Some(declarations) = tool.get("functionDeclarations") {
                    if let Some(arr) = declarations.as_array() {
                        return Some(arr.iter().filter_map(|decl| {
                            let name = decl.get("name")?.as_str()?.to_string();
                            let description = decl
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("")
                                .to_string();
                            let parameters = decl.get("parameters").cloned();

                            Some(ResponsesTool {
                                tool_type: "function".to_string(),
                                name: Some(name),
                                description: Some(description),
                                parameters,
                            })
                        }).collect::<Vec<_>>());
                    }
                }
                None
            })
            .flatten()
            .collect();

        // Add native tools for Code and Agentic tiers
        // These are GPT-5.1 built-in tools with 35% fewer failures for file ops
        if self.model == OpenAIModel::Gpt51CodexMax {
            responses_tools.push(native_apply_patch_tool());
            responses_tools.push(native_shell_tool());
        }

        responses_tools
    }

    /// Validate API key with a minimal request
    pub async fn validate_api_key(&self) -> Result<()> {
        debug!("Validating OpenAI API key");

        let request = ResponsesRequest {
            model: self.model.as_str().to_string(),
            input: ResponsesInput::Text("test".to_string()),
            instructions: None,
            tools: None,
            tool_choice: None,
            max_output_tokens: Some(1),
            stream: None,
            store: Some(false),
            reasoning: None,
            previous_response_id: None,
        };

        let response = self
            .client
            .post(format!("{}/responses", Self::BASE_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            let error_msg = match status.as_u16() {
                401 => "Invalid OpenAI API key".to_string(),
                429 => "Rate limit exceeded".to_string(),
                _ => format!("API validation failed ({}): {}", status, error_text),
            };
            return Err(anyhow!(error_msg));
        }

        info!("OpenAI API key validation successful");
        Ok(())
    }

    /// Send a Responses API request
    async fn send_request(&self, request: &ResponsesRequest) -> Result<ResponsesResponse> {
        debug!(
            "Sending Responses API request to OpenAI {} model",
            self.model,
        );

        let response = self
            .client
            .post(format!("{}/responses", Self::BASE_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(self.timeout)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();

            // Try to parse as OpenAI error response
            if let Ok(error_resp) = serde_json::from_str::<ErrorResponse>(&error_text) {
                return Err(anyhow!(
                    "OpenAI API error ({}): {}",
                    error_resp.error.error_type,
                    error_resp.error.message
                ));
            }

            return Err(anyhow!("OpenAI API returned {}: {}", status, error_text));
        }

        let response_body: ResponsesResponse = response.json().await?;
        Ok(response_body)
    }

    /// Parse Responses API response into internal format
    fn parse_response(&self, response: ResponsesResponse, latency_ms: i64) -> Result<Response> {
        // Try output_text first (convenience field)
        let content = if let Some(text) = response.output_text {
            text
        } else {
            // Extract from output array
            let mut text = String::new();
            for item in &response.output {
                if let OutputItem::Message { content, .. } = item {
                    for c in content {
                        if let OutputContent::OutputText { text: t, .. } = c {
                            text.push_str(t);
                        }
                    }
                }
            }
            text
        };

        let usage = response.usage.as_ref();
        let reasoning_tokens = usage
            .and_then(|u| u.output_tokens_details.as_ref())
            .map(|d| d.reasoning_tokens)
            .unwrap_or(0);

        let tokens = TokenUsage {
            input: usage.map(|u| u.input_tokens).unwrap_or(0),
            output: usage.map(|u| u.output_tokens).unwrap_or(0),
            reasoning: reasoning_tokens,
            cached: 0,
        };

        debug!(
            "OpenAI {} response: {} input, {} output tokens ({}ms)",
            self.model, tokens.input, tokens.output, latency_ms
        );

        Ok(Response {
            content,
            model: self.model.as_str().to_string(),
            tokens,
            latency_ms,
        })
    }

    /// Parse tool calling response from Responses API
    /// Handles both custom function calls and native tools (apply_patch, shell)
    fn parse_tool_response(
        &self,
        response: ResponsesResponse,
        latency_ms: i64,
    ) -> Result<ToolResponse> {
        // Get text output
        let text_output = response.output_text.clone().unwrap_or_default();

        // Extract function calls from output (including native tool calls)
        let function_calls: Vec<FunctionCall> = response
            .output
            .iter()
            .filter_map(|item| {
                match item {
                    OutputItem::FunctionCall {
                        id,
                        call_id,
                        name,
                        arguments,
                    } => {
                        // Parse arguments from JSON string
                        let args: Value = serde_json::from_str(arguments).unwrap_or_else(|e| {
                            warn!("Failed to parse tool call arguments: {} - {}", e, arguments);
                            Value::Object(serde_json::Map::new())
                        });

                        Some(FunctionCall {
                            id: id.clone().unwrap_or_else(|| call_id.clone()),
                            name: name.clone(),
                            arguments: args,
                        })
                    }
                    OutputItem::ApplyPatchCall { id, call_id, patch } => {
                        // Convert native apply_patch to FunctionCall format
                        // The tool router will handle the V4A patch parsing
                        debug!("Native apply_patch call: {} bytes", patch.len());
                        Some(FunctionCall {
                            id: id.clone().unwrap_or_else(|| call_id.clone()),
                            name: "__native_apply_patch".to_string(),
                            arguments: serde_json::json!({ "patch": patch }),
                        })
                    }
                    OutputItem::ShellCall { id, call_id, command, workdir, timeout } => {
                        // Convert native shell to FunctionCall format
                        debug!("Native shell call: {:?}", command);
                        Some(FunctionCall {
                            id: id.clone().unwrap_or_else(|| call_id.clone()),
                            name: "__native_shell".to_string(),
                            arguments: serde_json::json!({
                                "command": command,
                                "workdir": workdir,
                                "timeout": timeout.unwrap_or(120)
                            }),
                        })
                    }
                    _ => None,
                }
            })
            .collect();

        let usage = response.usage.as_ref();
        let reasoning_tokens = usage
            .and_then(|u| u.output_tokens_details.as_ref())
            .map(|d| d.reasoning_tokens)
            .unwrap_or(0);

        let tokens = TokenUsage {
            input: usage.map(|u| u.input_tokens).unwrap_or(0),
            output: usage.map(|u| u.output_tokens).unwrap_or(0),
            reasoning: reasoning_tokens,
            cached: 0,
        };

        debug!(
            "OpenAI {} tool response: {} calls, {} input, {} output tokens",
            self.model,
            function_calls.len(),
            tokens.input,
            tokens.output
        );

        let response_id = response.id.clone();
        let raw = serde_json::to_value(&response).unwrap_or(Value::Null);

        Ok(ToolResponse {
            id: response_id,
            text_output,
            function_calls,
            tokens,
            latency_ms,
            raw_response: raw,
        })
    }

    /// Calculate cost for a response
    pub fn calculate_cost(&self, tokens: &TokenUsage) -> f64 {
        OpenAIPricing::calculate_cost(self.model, tokens.input, tokens.output)
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn name(&self) -> &'static str {
        match (self.model, self.reasoning_effort) {
            (OpenAIModel::Gpt51, _) => "openai-gpt51",
            (OpenAIModel::Gpt51Mini, _) => "openai-gpt51-mini",
            (OpenAIModel::Gpt51CodexMax, Some(ReasoningEffort::XHigh)) => "openai-codex-max-agentic",
            (OpenAIModel::Gpt51CodexMax, _) => "openai-codex-max",
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn chat(&self, messages: Vec<Message>, system: String) -> Result<Response> {
        let start = Instant::now();

        let input = self.messages_to_input(&messages, &system);

        let request = ResponsesRequest {
            model: self.model.as_str().to_string(),
            input,
            instructions: if system.is_empty() { None } else { Some(system) },
            tools: None,
            tool_choice: None,
            max_output_tokens: None,
            stream: None,
            store: Some(false),
            reasoning: self.build_reasoning_config(),
            previous_response_id: None,
        };

        let response = self.send_request(&request).await?;
        let latency_ms = start.elapsed().as_millis() as i64;

        self.parse_response(response, latency_ms)
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        _context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        let start = Instant::now();

        let input = self.messages_to_input(&messages, &system);
        let responses_tools = self.tools_to_responses(&tools);

        info!(
            "OpenAI {} chat_with_tools: {} tools",
            self.model,
            responses_tools.len()
        );

        let request = ResponsesRequest {
            model: self.model.as_str().to_string(),
            input,
            instructions: if system.is_empty() { None } else { Some(system) },
            tools: if responses_tools.is_empty() {
                None
            } else {
                Some(responses_tools)
            },
            tool_choice: Some(serde_json::json!("auto")),
            max_output_tokens: None,
            stream: None,
            store: Some(false),
            reasoning: self.build_reasoning_config(),
            previous_response_id: None,
        };

        let response = self.send_request(&request).await?;
        let latency_ms = start.elapsed().as_millis() as i64;

        self.parse_tool_response(response, latency_ms)
    }

    async fn stream(
        &self,
        messages: Vec<Message>,
        system: String,
    ) -> Result<Box<dyn futures::Stream<Item = Result<String>> + Send + Unpin>> {
        let input = self.messages_to_input(&messages, &system);

        let request = ResponsesRequest {
            model: self.model.as_str().to_string(),
            input,
            instructions: if system.is_empty() { None } else { Some(system) },
            tools: None,
            tool_choice: None,
            max_output_tokens: None,
            stream: Some(true),
            store: Some(false),
            reasoning: self.build_reasoning_config(),
            previous_response_id: None,
        };

        let response = self
            .client
            .post(format!("{}/responses", Self::BASE_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(self.timeout)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI streaming failed ({}): {}", status, error_text));
        }

        // Convert byte stream to text stream, parsing SSE events
        let byte_stream = response.bytes_stream();

        let stream = byte_stream
            .filter_map(|result| async move {
                match result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        let mut content = String::new();

                        // Parse SSE format for Responses API
                        // Events are: event: <type>\ndata: <json>
                        for line in text.lines() {
                            if let Some(data) = line.strip_prefix("data: ") {
                                if data == "[DONE]" {
                                    continue;
                                }
                                // Try to parse as ResponsesStreamEvent
                                if let Ok(event) =
                                    serde_json::from_str::<ResponsesStreamEvent>(data)
                                {
                                    // Handle response.output_text.delta events
                                    if event.event_type == "response.output_text.delta" {
                                        if let Some(delta) = &event.delta {
                                            content.push_str(delta);
                                        }
                                    }
                                }
                            }
                        }

                        if content.is_empty() {
                            None
                        } else {
                            Some(Ok(content))
                        }
                    }
                    Err(e) => Some(Err(anyhow!("Stream error: {}", e))),
                }
            })
            .boxed();

        Ok(Box::new(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_creation() {
        let provider = OpenAIProvider::gpt51("test-key".to_string());
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        assert_eq!(provider.model(), OpenAIModel::Gpt51);
        assert!(provider.is_available());
    }

    #[test]
    fn test_provider_creation_mini() {
        let provider = OpenAIProvider::gpt51_mini("test-key".to_string());
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        assert_eq!(provider.model(), OpenAIModel::Gpt51Mini);
        assert_eq!(provider.name(), "openai-gpt51-mini");
    }

    #[test]
    fn test_provider_requires_key() {
        let provider = OpenAIProvider::new("".to_string(), OpenAIModel::Gpt51);
        assert!(provider.is_err());
    }

    #[test]
    fn test_cost_calculation() {
        let provider = OpenAIProvider::gpt51("test-key".to_string()).unwrap();
        let tokens = TokenUsage {
            input: 100_000,
            output: 10_000,
            reasoning: 0,
            cached: 0,
        };

        let cost = provider.calculate_cost(&tokens);
        // GPT-5.1: 0.1 * $1.25 + 0.01 * $10 = $0.225
        assert!((cost - 0.225).abs() < 0.001);
    }

    #[test]
    fn test_codex_models() {
        let fast = OpenAIProvider::gpt51_mini("test-key".to_string()).unwrap();
        assert_eq!(fast.model().as_str(), "gpt-5.1-codex-mini");
        // Fast tier uses "none" reasoning for lowest latency/cost
        assert_eq!(fast.reasoning_effort(), Some(ReasoningEffort::None));

        let code = OpenAIProvider::codex_max("test-key".to_string()).unwrap();
        assert_eq!(code.model().as_str(), "gpt-5.1-codex-max");
        assert_eq!(code.reasoning_effort(), Some(ReasoningEffort::High));

        let agentic = OpenAIProvider::codex_max_agentic("test-key".to_string()).unwrap();
        assert_eq!(agentic.model().as_str(), "gpt-5.1-codex-max");
        assert_eq!(agentic.reasoning_effort(), Some(ReasoningEffort::XHigh));
    }
}
