// crates/mira-server/src/llm/openai/client.rs
// OpenAI GPT-5.2 client using the Responses API

use crate::llm::deepseek::{ChatResult, FunctionCall, Message, Tool, ToolCall, Usage};
use crate::llm::provider::{LlmClient, Provider};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::{debug, info, instrument, Span};
use uuid::Uuid;

// GPT-5.2 uses the Responses API for better CoT handling
const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

/// Request timeout - allow time for complex reasoning
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default model
const DEFAULT_MODEL: &str = "gpt-5.2";

// ============================================================================
// Responses API Request Types
// ============================================================================

/// GPT-5.2 Responses API request
#[derive(Debug, Serialize)]
struct ResponsesRequest {
    model: String,
    input: Vec<InputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,  // "auto" | "required" | "none"
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<TextConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    /// Store the response for use with previous_response_id
    #[serde(skip_serializing_if = "Option::is_none")]
    store: Option<bool>,
    /// Reference to previous response to continue the conversation
    /// This preserves reasoning context across turns
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_response_id: Option<String>,
}

/// Input item for Responses API (can be message, function call, or function result)
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum InputItem {
    Message(InputMessage),
    FunctionCall(FunctionCallInput),
    ToolResult(ToolResultInput),
}

/// Function call input (to continue after a function call)
#[derive(Debug, Serialize)]
struct FunctionCallInput {
    #[serde(rename = "type")]
    item_type: String, // "function_call"
    id: String,
    call_id: String, // Same as id for function calls
    name: String,
    arguments: String,
}

/// Message input
#[derive(Debug, Serialize)]
struct InputMessage {
    role: String,
    content: String,
}

/// Tool result input
#[derive(Debug, Serialize)]
struct ToolResultInput {
    #[serde(rename = "type")]
    item_type: String, // "function_call_output"
    call_id: String,
    output: String,
}

/// Tool definition for Responses API
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ResponsesTool {
    Function(FunctionTool),
    BuiltIn(BuiltInTool),
}

/// Function tool definition (Responses API uses flat structure)
#[derive(Debug, Serialize)]
struct FunctionTool {
    #[serde(rename = "type")]
    tool_type: String, // "function"
    name: String,
    description: String,
    parameters: Value,
}

/// Built-in tool (like web_search)
#[derive(Debug, Serialize)]
struct BuiltInTool {
    #[serde(rename = "type")]
    tool_type: String, // "web_search"
}

// Tool choice is just a string: "auto" | "required" | "none"
// or an object for specific tool forcing - but we use string form

/// Reasoning configuration for GPT-5.2
#[derive(Debug, Serialize)]
struct ReasoningConfig {
    effort: String, // "none" | "low" | "medium" | "high" | "xhigh"
}

/// Text output configuration
#[derive(Debug, Serialize)]
struct TextConfig {
    verbosity: String, // "low" | "medium" | "high"
}

// ============================================================================
// Responses API Response Types
// ============================================================================

/// GPT-5.2 Responses API response
#[derive(Debug, Deserialize)]
struct ResponsesResponse {
    id: String,
    output: Vec<OutputItem>,
    usage: Option<ResponsesUsage>,
}

/// Output item (can be message, reasoning, or tool call)
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum OutputItem {
    #[serde(rename = "message")]
    Message(MessageOutput),
    #[serde(rename = "reasoning")]
    Reasoning(ReasoningOutput),
    #[serde(rename = "function_call")]
    FunctionCall(FunctionCallOutput),
}

/// Message output
#[derive(Debug, Deserialize)]
struct MessageOutput {
    content: Vec<ContentPart>,
}

/// Content part in message
#[derive(Debug, Deserialize)]
struct ContentPart {
    #[serde(rename = "type")]
    part_type: String,
    #[serde(default)]
    text: Option<String>,
}

/// Reasoning output (chain of thought)
#[derive(Debug, Deserialize)]
struct ReasoningOutput {
    #[serde(default)]
    summary: Option<Vec<ContentPart>>,
}

/// Function call output
#[derive(Debug, Deserialize)]
struct FunctionCallOutput {
    id: String,
    call_id: String,
    name: String,
    arguments: String,
}

/// Usage statistics
#[derive(Debug, Deserialize)]
struct ResponsesUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    reasoning_tokens: Option<u32>,
}

// ============================================================================
// Client Implementation
// ============================================================================

/// OpenAI GPT-5.2 client using Responses API
pub struct OpenAiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
    /// Reasoning effort level
    reasoning_effort: String,
    /// Enable web search tool
    enable_web_search: bool,
}

impl OpenAiClient {
    /// Create a new OpenAI client with default model
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, DEFAULT_MODEL.to_string())
    }

    /// Create a new OpenAI client with custom model
    pub fn with_model(api_key: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self::with_http_client(api_key, model, client)
    }

    /// Create a new OpenAI client with a shared HTTP client
    pub fn with_http_client(api_key: String, model: String, client: reqwest::Client) -> Self {
        Self {
            api_key,
            model,
            client,
            reasoning_effort: "medium".to_string(),
            enable_web_search: true,
        }
    }

    /// Enable or disable web search
    pub fn set_web_search(&mut self, enabled: bool) {
        self.enable_web_search = enabled;
    }

    /// Convert internal Message to Responses API input items
    /// Returns a Vec because assistant messages with tool_calls produce multiple items
    fn convert_message(msg: &Message) -> Vec<InputItem> {
        match msg.role.as_str() {
            "system" | "user" => {
                vec![InputItem::Message(InputMessage {
                    role: msg.role.clone(),
                    content: msg.content.clone().unwrap_or_default(),
                })]
            }
            "assistant" => {
                let mut items = Vec::new();

                // If assistant has tool_calls, emit function_call items for each
                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        items.push(InputItem::FunctionCall(FunctionCallInput {
                            item_type: "function_call".to_string(),
                            // Use item_id (fc_ prefix) for the id field, fall back to id
                            id: tc.item_id.clone().unwrap_or_else(|| tc.id.clone()),
                            // Use id (call_ prefix) for correlation
                            call_id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.clone(),
                        }));
                    }
                } else {
                    // Regular assistant message without tool calls
                    items.push(InputItem::Message(InputMessage {
                        role: "assistant".to_string(),
                        content: msg.content.clone().unwrap_or_default(),
                    }));
                }
                items
            }
            "tool" => {
                // Tool results in Responses API format
                vec![InputItem::ToolResult(ToolResultInput {
                    item_type: "function_call_output".to_string(),
                    call_id: msg.tool_call_id.clone().unwrap_or_default(),
                    output: msg.content.clone().unwrap_or_default(),
                })]
            }
            _ => vec![],
        }
    }

    /// Convert internal Tool to Responses API format (flat structure)
    fn convert_tool(tool: &Tool) -> ResponsesTool {
        ResponsesTool::Function(FunctionTool {
            tool_type: "function".to_string(),
            name: tool.function.name.clone(),
            description: tool.function.description.clone(),
            parameters: tool.function.parameters.clone(),
        })
    }

    /// Extract text content from output items
    fn extract_content(output: &[OutputItem]) -> Option<String> {
        let mut texts = Vec::new();
        for item in output {
            if let OutputItem::Message(msg) = item {
                for part in &msg.content {
                    if part.part_type == "text" || part.part_type == "output_text" {
                        if let Some(ref text) = part.text {
                            texts.push(text.clone());
                        }
                    }
                }
            }
        }
        if texts.is_empty() {
            None
        } else {
            Some(texts.join(""))
        }
    }

    /// Extract reasoning summary from output items
    fn extract_reasoning(output: &[OutputItem]) -> Option<String> {
        for item in output {
            if let OutputItem::Reasoning(reasoning) = item {
                if let Some(ref summary) = reasoning.summary {
                    let texts: Vec<&str> = summary
                        .iter()
                        .filter_map(|p| p.text.as_deref())
                        .collect();
                    if !texts.is_empty() {
                        return Some(texts.join(""));
                    }
                }
            }
        }
        None
    }

    /// Extract tool calls from output items
    fn extract_tool_calls(output: &[OutputItem]) -> Option<Vec<ToolCall>> {
        let calls: Vec<ToolCall> = output
            .iter()
            .filter_map(|item| {
                if let OutputItem::FunctionCall(fc) = item {
                    Some(ToolCall {
                        // call_id for correlation with function_call_output
                        id: fc.call_id.clone(),
                        // item id (fc_ prefix) for FunctionCallInput
                        item_id: Some(fc.id.clone()),
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: fc.name.clone(),
                            arguments: fc.arguments.clone(),
                        },
                        thought_signature: None, // OpenAI doesn't use thought signatures
                    })
                } else {
                    None
                }
            })
            .collect();

        if calls.is_empty() {
            None
        } else {
            Some(calls)
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiClient {
    fn provider_type(&self) -> Provider {
        Provider::OpenAi
    }

    fn model_name(&self) -> String {
        self.model.clone()
    }

    fn supports_stateful(&self) -> bool {
        true
    }

    #[instrument(skip(self, messages, tools), fields(request_id, model = %self.model, message_count = messages.len()))]
    async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        Span::current().record("request_id", &request_id);

        info!(
            request_id = %request_id,
            message_count = messages.len(),
            tool_count = tools.as_ref().map(|t| t.len()).unwrap_or(0),
            model = %self.model,
            reasoning_effort = %self.reasoning_effort,
            "Starting GPT-5.2 Responses API request"
        );

        // Convert messages to Responses API input format
        // Uses flat_map because each message can produce multiple input items
        // (e.g., assistant messages with tool_calls produce function_call items)
        let input: Vec<InputItem> = messages
            .iter()
            .flat_map(Self::convert_message)
            .collect();

        // Build tools list - include web_search if enabled
        let mut api_tools: Vec<ResponsesTool> = Vec::new();

        // Add web_search as a built-in tool
        if self.enable_web_search {
            api_tools.push(ResponsesTool::BuiltIn(BuiltInTool {
                tool_type: "web_search".to_string(),
            }));
        }

        // Add custom function tools
        if let Some(ref custom_tools) = tools {
            for tool in custom_tools {
                api_tools.push(Self::convert_tool(tool));
            }
        }

        let request = ResponsesRequest {
            model: self.model.clone(),
            input,
            tools: if api_tools.is_empty() { None } else { Some(api_tools) },
            tool_choice: if tools.is_some() || self.enable_web_search {
                Some("auto".to_string())
            } else {
                None
            },
            reasoning: Some(ReasoningConfig {
                effort: self.reasoning_effort.clone(),
            }),
            text: Some(TextConfig {
                verbosity: "medium".to_string(),
            }),
            max_output_tokens: Some(8192),
            store: Some(true), // Enable stateful conversations
            previous_response_id: None,
        };

        let body = serde_json::to_string(&request)?;
        debug!(request_id = %request_id, "GPT-5.2 request: {}", body);

        let mut attempts = 0;
        let max_attempts = 3;
        let mut backoff = Duration::from_secs(1);

        loop {
            let response_result = self
                .client
                .post(OPENAI_RESPONSES_URL)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .body(body.clone())
                .send()
                .await;

            match response_result {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        let error_body = response.text().await.unwrap_or_default();
                        
                        // Check for transient errors
                        if attempts < max_attempts && (status.as_u16() == 429 || status.is_server_error()) {
                            tracing::warn!(
                                request_id = %request_id,
                                status = %status,
                                error = %error_body,
                                "Transient error from OpenAI, retrying in {:?}...",
                                backoff
                            );
                            tokio::time::sleep(backoff).await;
                            attempts += 1;
                            backoff *= 2;
                            continue;
                        }

                        return Err(anyhow!("GPT-5.2 API error {}: {}", status, error_body));
                    }

                    let data: ResponsesResponse = response
                        .json()
                        .await
                        .map_err(|e| anyhow!("Failed to parse GPT-5.2 response: {}", e))?;

                    let duration_ms = start_time.elapsed().as_millis() as u64;

                    // Extract content, reasoning, and tool calls from output
                    let content = Self::extract_content(&data.output);
                    let reasoning_content = Self::extract_reasoning(&data.output);
                    let tool_calls = Self::extract_tool_calls(&data.output);

                    // Convert usage
                    let usage = data.usage.map(|u| Usage {
                        prompt_tokens: u.input_tokens,
                        completion_tokens: u.output_tokens + u.reasoning_tokens.unwrap_or(0),
                        total_tokens: u.input_tokens + u.output_tokens + u.reasoning_tokens.unwrap_or(0),
                        prompt_cache_hit_tokens: None,
                        prompt_cache_miss_tokens: None,
                    });

                    // Log usage stats
                    if let Some(ref u) = usage {
                        info!(
                            request_id = %request_id,
                            prompt_tokens = u.prompt_tokens,
                            completion_tokens = u.completion_tokens,
                            total_tokens = u.total_tokens,
                            "GPT-5.2 usage stats"
                        );
                    }

                    // Log tool calls if any
                    if let Some(ref tcs) = tool_calls {
                        info!(
                            request_id = %request_id,
                            tool_count = tcs.len(),
                            tools = ?tcs.iter().map(|tc| &tc.function.name).collect::<Vec<_>>(),
                            "GPT-5.2 requested tool calls"
                        );
                        for tc in tcs {
                            let args: serde_json::Value =
                                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
                            debug!(
                                request_id = %request_id,
                                tool = %tc.function.name,
                                call_id = %tc.id,
                                args = %args,
                                "Tool call"
                            );
                        }
                    }

                    info!(
                        request_id = %request_id,
                        duration_ms = duration_ms,
                        content_len = content.as_ref().map(|c| c.len()).unwrap_or(0),
                        reasoning_len = reasoning_content.as_ref().map(|r| r.len()).unwrap_or(0),
                        tool_calls = tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
                        "GPT-5.2 chat complete"
                    );

                    return Ok(ChatResult {
                        request_id: data.id,
                        content,
                        reasoning_content,
                        tool_calls,
                        usage,
                        duration_ms,
                    });
                }
                Err(e) => {
                    if attempts < max_attempts {
                        tracing::warn!(
                            request_id = %request_id,
                            error = %e,
                            "Request failed, retrying in {:?}...",
                            backoff
                        );
                        tokio::time::sleep(backoff).await;
                        attempts += 1;
                        backoff *= 2;
                        continue;
                    }
                    return Err(anyhow!("GPT-5.2 request failed after retries: {}", e));
                }
            }
        }
    }

    /// Stateful chat that uses previous_response_id to preserve reasoning context.
    /// When previous_response_id is provided, we only send NEW messages (typically
    /// just tool results), as OpenAI's stored response contains the full context
    /// including reasoning items.
    ///
    /// IMPORTANT: When previous_response_id is set, this method expects `messages`
    /// to contain ONLY the new tool results for the current turn, not the full
    /// conversation history. The caller (e.g., consult_expert) should track and
    /// pass only new messages.
    #[instrument(skip(self, messages, tools), fields(request_id, model = %self.model))]
    async fn chat_stateful(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
        previous_response_id: Option<&str>,
    ) -> Result<ChatResult> {
        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        Span::current().record("request_id", &request_id);

        // When we have a previous_response_id, we expect the caller to pass only
        // new messages (tool results from the current turn). We still filter to
        // ensure only tool messages are sent.
        let input: Vec<InputItem> = if previous_response_id.is_some() {
            // With previous_response_id, only send tool messages (function_call_output)
            // Caller should have already filtered to new messages only
            messages
                .iter()
                .filter(|m| m.role == "tool")
                .flat_map(Self::convert_message)
                .collect()
        } else {
            // No previous response - send all messages
            messages
                .iter()
                .flat_map(Self::convert_message)
                .collect()
        };

        info!(
            request_id = %request_id,
            message_count = messages.len(),
            input_count = input.len(),
            has_previous = previous_response_id.is_some(),
            model = %self.model,
            "GPT-5.2 stateful request"
        );

        // Build tools list
        let mut api_tools: Vec<ResponsesTool> = Vec::new();
        if self.enable_web_search {
            api_tools.push(ResponsesTool::BuiltIn(BuiltInTool {
                tool_type: "web_search".to_string(),
            }));
        }
        if let Some(ref custom_tools) = tools {
            for tool in custom_tools {
                api_tools.push(Self::convert_tool(tool));
            }
        }

        let request = ResponsesRequest {
            model: self.model.clone(),
            input,
            tools: if api_tools.is_empty() { None } else { Some(api_tools) },
            tool_choice: if tools.is_some() || self.enable_web_search {
                Some("auto".to_string())
            } else {
                None
            },
            reasoning: Some(ReasoningConfig {
                effort: self.reasoning_effort.clone(),
            }),
            text: Some(TextConfig {
                verbosity: "medium".to_string(),
            }),
            max_output_tokens: Some(8192),
            store: Some(true),
            previous_response_id: previous_response_id.map(|s| s.to_string()),
        };

        let body = serde_json::to_string(&request)?;
        debug!(request_id = %request_id, "GPT-5.2 stateful request: {}", body);

        let mut attempts = 0;
        let max_attempts = 3;
        let mut backoff = Duration::from_secs(1);

        loop {
            let response_result = self
                .client
                .post(OPENAI_RESPONSES_URL)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .body(body.clone())
                .send()
                .await;

            match response_result {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        let error_body = response.text().await.unwrap_or_default();
                        
                        // Check for transient errors
                        if attempts < max_attempts && (status.as_u16() == 429 || status.is_server_error()) {
                            tracing::warn!(
                                request_id = %request_id,
                                status = %status,
                                error = %error_body,
                                "Transient error from OpenAI, retrying in {:?}...",
                                backoff
                            );
                            tokio::time::sleep(backoff).await;
                            attempts += 1;
                            backoff *= 2;
                            continue;
                        }
                        
                        return Err(anyhow!("GPT-5.2 API error {}: {}", status, error_body));
                    }

                    let data: ResponsesResponse = response
                        .json()
                        .await
                        .map_err(|e| anyhow!("Failed to parse GPT-5.2 response: {}", e))?;

                    let duration_ms = start_time.elapsed().as_millis() as u64;

                    let content = Self::extract_content(&data.output);
                    let reasoning_content = Self::extract_reasoning(&data.output);
                    let tool_calls = Self::extract_tool_calls(&data.output);

                    let usage = data.usage.map(|u| Usage {
                        prompt_tokens: u.input_tokens,
                        completion_tokens: u.output_tokens + u.reasoning_tokens.unwrap_or(0),
                        total_tokens: u.input_tokens + u.output_tokens + u.reasoning_tokens.unwrap_or(0),
                        prompt_cache_hit_tokens: None,
                        prompt_cache_miss_tokens: None,
                    });

                    info!(
                        request_id = %request_id,
                        duration_ms = duration_ms,
                        response_id = %data.id,
                        tool_calls = tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
                        "GPT-5.2 stateful chat complete"
                    );

                    return Ok(ChatResult {
                        request_id: data.id, // This is the response ID for chaining
                        content,
                        reasoning_content,
                        tool_calls,
                        usage,
                        duration_ms,
                    });
                }
                Err(e) => {
                    if attempts < max_attempts {
                        tracing::warn!(
                            request_id = %request_id,
                            error = %e,
                            "Request failed, retrying in {:?}...",
                            backoff
                        );
                        tokio::time::sleep(backoff).await;
                        attempts += 1;
                        backoff *= 2;
                        continue;
                    }
                    return Err(anyhow!("GPT-5.2 request failed after retries: {}", e));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // Constants tests
    // ============================================================================

    #[test]
    fn test_default_model() {
        assert_eq!(DEFAULT_MODEL, "gpt-5.2");
    }

    #[test]
    fn test_responses_url() {
        assert_eq!(OPENAI_RESPONSES_URL, "https://api.openai.com/v1/responses");
    }

    #[test]
    fn test_timeouts() {
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(300));
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(30));
    }

    // ============================================================================
    // InputItem serialization tests
    // ============================================================================

    #[test]
    fn test_input_message_serialize() {
        let item = InputItem::Message(InputMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        });
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Hello\""));
    }

    #[test]
    fn test_function_call_input_serialize() {
        let item = InputItem::FunctionCall(FunctionCallInput {
            item_type: "function_call".to_string(),
            id: "fc_123".to_string(),
            call_id: "call_123".to_string(),
            name: "search".to_string(),
            arguments: r#"{"query":"test"}"#.to_string(),
        });
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("function_call"));
        assert!(json.contains("search"));
    }

    #[test]
    fn test_tool_result_input_serialize() {
        let item = InputItem::ToolResult(ToolResultInput {
            item_type: "function_call_output".to_string(),
            call_id: "call_123".to_string(),
            output: "Found 5 results".to_string(),
        });
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("function_call_output"));
        assert!(json.contains("call_123"));
    }

    // ============================================================================
    // ResponsesTool serialization tests
    // ============================================================================

    #[test]
    fn test_function_tool_serialize() {
        let tool = ResponsesTool::Function(FunctionTool {
            tool_type: "function".to_string(),
            name: "search".to_string(),
            description: "Search for things".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        });
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("\"type\":\"function\""));
        assert!(json.contains("search"));
    }

    #[test]
    fn test_builtin_tool_serialize() {
        let tool = ResponsesTool::BuiltIn(BuiltInTool {
            tool_type: "web_search".to_string(),
        });
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("web_search"));
    }

    // ============================================================================
    // Config serialization tests
    // ============================================================================

    #[test]
    fn test_reasoning_config_serialize() {
        let config = ReasoningConfig {
            effort: "high".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"effort\":\"high\""));
    }

    #[test]
    fn test_text_config_serialize() {
        let config = TextConfig {
            verbosity: "medium".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"verbosity\":\"medium\""));
    }

    // ============================================================================
    // OutputItem deserialization tests
    // ============================================================================

    #[test]
    fn test_message_output_deserialize() {
        let json = r#"{"type": "message", "content": [{"type": "text", "text": "Hello"}]}"#;
        let item: OutputItem = serde_json::from_str(json).unwrap();
        match item {
            OutputItem::Message(msg) => {
                assert_eq!(msg.content.len(), 1);
                assert_eq!(msg.content[0].text, Some("Hello".to_string()));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[test]
    fn test_reasoning_output_deserialize() {
        let json = r#"{"type": "reasoning", "summary": [{"type": "text", "text": "Thinking..."}]}"#;
        let item: OutputItem = serde_json::from_str(json).unwrap();
        match item {
            OutputItem::Reasoning(r) => {
                assert!(r.summary.is_some());
            }
            _ => panic!("Expected Reasoning"),
        }
    }

    #[test]
    fn test_function_call_output_deserialize() {
        let json = r#"{"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "search", "arguments": "{}"}"#;
        let item: OutputItem = serde_json::from_str(json).unwrap();
        match item {
            OutputItem::FunctionCall(fc) => {
                assert_eq!(fc.name, "search");
                assert_eq!(fc.call_id, "call_1");
            }
            _ => panic!("Expected FunctionCall"),
        }
    }

    // ============================================================================
    // extract_content tests
    // ============================================================================

    #[test]
    fn test_extract_content_single_message() {
        let output = vec![OutputItem::Message(MessageOutput {
            content: vec![ContentPart {
                part_type: "text".to_string(),
                text: Some("Hello world".to_string()),
            }],
        })];
        assert_eq!(
            OpenAiClient::extract_content(&output),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn test_extract_content_output_text_type() {
        let output = vec![OutputItem::Message(MessageOutput {
            content: vec![ContentPart {
                part_type: "output_text".to_string(),
                text: Some("Output text".to_string()),
            }],
        })];
        assert_eq!(
            OpenAiClient::extract_content(&output),
            Some("Output text".to_string())
        );
    }

    #[test]
    fn test_extract_content_multiple_parts() {
        let output = vec![OutputItem::Message(MessageOutput {
            content: vec![
                ContentPart {
                    part_type: "text".to_string(),
                    text: Some("Hello ".to_string()),
                },
                ContentPart {
                    part_type: "text".to_string(),
                    text: Some("world".to_string()),
                },
            ],
        })];
        assert_eq!(
            OpenAiClient::extract_content(&output),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn test_extract_content_empty_output() {
        let output: Vec<OutputItem> = vec![];
        assert_eq!(OpenAiClient::extract_content(&output), None);
    }

    #[test]
    fn test_extract_content_no_text() {
        let output = vec![OutputItem::Message(MessageOutput {
            content: vec![ContentPart {
                part_type: "image".to_string(),
                text: None,
            }],
        })];
        assert_eq!(OpenAiClient::extract_content(&output), None);
    }

    // ============================================================================
    // extract_reasoning tests
    // ============================================================================

    #[test]
    fn test_extract_reasoning_with_summary() {
        let output = vec![OutputItem::Reasoning(ReasoningOutput {
            summary: Some(vec![ContentPart {
                part_type: "text".to_string(),
                text: Some("Thinking about the problem...".to_string()),
            }]),
        })];
        assert_eq!(
            OpenAiClient::extract_reasoning(&output),
            Some("Thinking about the problem...".to_string())
        );
    }

    #[test]
    fn test_extract_reasoning_no_summary() {
        let output = vec![OutputItem::Reasoning(ReasoningOutput { summary: None })];
        assert_eq!(OpenAiClient::extract_reasoning(&output), None);
    }

    #[test]
    fn test_extract_reasoning_empty_summary() {
        let output = vec![OutputItem::Reasoning(ReasoningOutput {
            summary: Some(vec![]),
        })];
        assert_eq!(OpenAiClient::extract_reasoning(&output), None);
    }

    #[test]
    fn test_extract_reasoning_multiple_parts() {
        let output = vec![OutputItem::Reasoning(ReasoningOutput {
            summary: Some(vec![
                ContentPart {
                    part_type: "text".to_string(),
                    text: Some("First ".to_string()),
                },
                ContentPart {
                    part_type: "text".to_string(),
                    text: Some("second".to_string()),
                },
            ]),
        })];
        assert_eq!(
            OpenAiClient::extract_reasoning(&output),
            Some("First second".to_string())
        );
    }

    // ============================================================================
    // extract_tool_calls tests
    // ============================================================================

    #[test]
    fn test_extract_tool_calls_single() {
        let output = vec![OutputItem::FunctionCall(FunctionCallOutput {
            id: "fc_123".to_string(),
            call_id: "call_123".to_string(),
            name: "search".to_string(),
            arguments: r#"{"query":"test"}"#.to_string(),
        })];
        let calls = OpenAiClient::extract_tool_calls(&output).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "search");
        assert_eq!(calls[0].id, "call_123");
        assert_eq!(calls[0].item_id, Some("fc_123".to_string()));
    }

    #[test]
    fn test_extract_tool_calls_multiple() {
        let output = vec![
            OutputItem::FunctionCall(FunctionCallOutput {
                id: "fc_1".to_string(),
                call_id: "call_1".to_string(),
                name: "search".to_string(),
                arguments: "{}".to_string(),
            }),
            OutputItem::FunctionCall(FunctionCallOutput {
                id: "fc_2".to_string(),
                call_id: "call_2".to_string(),
                name: "read".to_string(),
                arguments: "{}".to_string(),
            }),
        ];
        let calls = OpenAiClient::extract_tool_calls(&output).unwrap();
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn test_extract_tool_calls_none_when_no_calls() {
        let output = vec![OutputItem::Message(MessageOutput {
            content: vec![ContentPart {
                part_type: "text".to_string(),
                text: Some("Just text".to_string()),
            }],
        })];
        assert_eq!(OpenAiClient::extract_tool_calls(&output), None);
    }

    // ============================================================================
    // convert_message tests
    // ============================================================================

    #[test]
    fn test_convert_message_user() {
        let msg = Message {
            role: "user".to_string(),
            content: Some("Hello".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        };
        let items = OpenAiClient::convert_message(&msg);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_convert_message_system() {
        let msg = Message {
            role: "system".to_string(),
            content: Some("You are helpful".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        };
        let items = OpenAiClient::convert_message(&msg);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_convert_message_assistant_no_tools() {
        let msg = Message {
            role: "assistant".to_string(),
            content: Some("I'll help you".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        };
        let items = OpenAiClient::convert_message(&msg);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_convert_message_assistant_with_tools() {
        let msg = Message {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: None,
            tool_calls: Some(vec![
                ToolCall {
                    id: "call_1".to_string(),
                    item_id: Some("fc_1".to_string()),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: "search".to_string(),
                        arguments: "{}".to_string(),
                    },
                    thought_signature: None,
                },
                ToolCall {
                    id: "call_2".to_string(),
                    item_id: None,
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: "read".to_string(),
                        arguments: "{}".to_string(),
                    },
                    thought_signature: None,
                },
            ]),
            tool_call_id: None,
        };
        let items = OpenAiClient::convert_message(&msg);
        assert_eq!(items.len(), 2); // One per tool call
    }

    #[test]
    fn test_convert_message_tool() {
        let msg = Message {
            role: "tool".to_string(),
            content: Some("Search results here".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some("call_123".to_string()),
        };
        let items = OpenAiClient::convert_message(&msg);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_convert_message_unknown_role() {
        let msg = Message {
            role: "unknown".to_string(),
            content: Some("Content".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        };
        let items = OpenAiClient::convert_message(&msg);
        assert!(items.is_empty());
    }

    // ============================================================================
    // convert_tool tests
    // ============================================================================

    #[test]
    fn test_convert_tool() {
        let tool = Tool {
            tool_type: "function".to_string(),
            function: crate::llm::deepseek::FunctionDef {
                name: "search".to_string(),
                description: "Search for things".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
        };
        let result = OpenAiClient::convert_tool(&tool);
        match result {
            ResponsesTool::Function(f) => {
                assert_eq!(f.name, "search");
                assert_eq!(f.tool_type, "function");
            }
            _ => panic!("Expected Function tool"),
        }
    }

    // ============================================================================
    // Client creation tests
    // ============================================================================

    #[test]
    fn test_client_new() {
        let client = OpenAiClient::new("test-key".to_string());
        assert_eq!(client.model, DEFAULT_MODEL);
        assert_eq!(client.reasoning_effort, "medium");
        assert!(client.enable_web_search);
    }

    #[test]
    fn test_client_with_model() {
        let client = OpenAiClient::with_model("test-key".to_string(), "gpt-5".to_string());
        assert_eq!(client.model, "gpt-5");
    }

    #[test]
    fn test_set_web_search() {
        let mut client = OpenAiClient::new("test-key".to_string());
        assert!(client.enable_web_search);
        client.set_web_search(false);
        assert!(!client.enable_web_search);
    }

    // ============================================================================
    // ResponsesUsage deserialization tests
    // ============================================================================

    #[test]
    fn test_responses_usage_deserialize() {
        let json = r#"{"input_tokens": 100, "output_tokens": 50, "reasoning_tokens": 25}"#;
        let usage: ResponsesUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.reasoning_tokens, Some(25));
    }

    #[test]
    fn test_responses_usage_deserialize_no_reasoning() {
        let json = r#"{"input_tokens": 100, "output_tokens": 50}"#;
        let usage: ResponsesUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.reasoning_tokens, None);
    }
}
