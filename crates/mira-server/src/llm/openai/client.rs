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
    tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<TextConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

/// Input item for Responses API (can be message or tool result)
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum InputItem {
    Message(InputMessage),
    ToolResult(ToolResultInput),
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

/// Function tool definition
#[derive(Debug, Serialize)]
struct FunctionTool {
    #[serde(rename = "type")]
    tool_type: String, // "function"
    function: FunctionDef,
}

/// Built-in tool (like web_search)
#[derive(Debug, Serialize)]
struct BuiltInTool {
    #[serde(rename = "type")]
    tool_type: String, // "web_search"
}

/// Function definition
#[derive(Debug, Serialize)]
struct FunctionDef {
    name: String,
    description: String,
    parameters: Value,
}

/// Tool choice configuration
#[derive(Debug, Serialize)]
struct ToolChoice {
    #[serde(rename = "type")]
    choice_type: String, // "auto" | "required" | "none"
}

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

        Self {
            api_key,
            model,
            client,
            reasoning_effort: "medium".to_string(), // Good default for expert tasks
            enable_web_search: true, // Enable by default for experts
        }
    }

    /// Convert internal Message to Responses API input
    fn convert_message(msg: &Message) -> Option<InputItem> {
        match msg.role.as_str() {
            "system" | "user" | "assistant" => {
                Some(InputItem::Message(InputMessage {
                    role: if msg.role == "assistant" { "assistant".to_string() } else { msg.role.clone() },
                    content: msg.content.clone().unwrap_or_default(),
                }))
            }
            "tool" => {
                // Tool results in Responses API format
                Some(InputItem::ToolResult(ToolResultInput {
                    item_type: "function_call_output".to_string(),
                    call_id: msg.tool_call_id.clone().unwrap_or_default(),
                    output: msg.content.clone().unwrap_or_default(),
                }))
            }
            _ => None,
        }
    }

    /// Convert internal Tool to Responses API format
    fn convert_tool(tool: &Tool) -> ResponsesTool {
        ResponsesTool::Function(FunctionTool {
            tool_type: "function".to_string(),
            function: FunctionDef {
                name: tool.function.name.clone(),
                description: tool.function.description.clone(),
                parameters: tool.function.parameters.clone(),
            },
        })
    }

    /// Extract text content from output items
    fn extract_content(output: &[OutputItem]) -> Option<String> {
        let mut texts = Vec::new();
        for item in output {
            if let OutputItem::Message(msg) = item {
                for part in &msg.content {
                    if part.part_type == "text" {
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
                        id: fc.id.clone(),
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: fc.name.clone(),
                            arguments: fc.arguments.clone(),
                        },
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
        let input: Vec<InputItem> = messages
            .iter()
            .filter_map(Self::convert_message)
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
                Some(ToolChoice {
                    choice_type: "auto".to_string(),
                })
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
        };

        debug!(request_id = %request_id, "GPT-5.2 request: {:?}", serde_json::to_string(&request)?);

        let response = self
            .client
            .post(OPENAI_RESPONSES_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow!("GPT-5.2 request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("GPT-5.2 API error {}: {}", status, body));
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

        Ok(ChatResult {
            request_id: data.id,
            content,
            reasoning_content,
            tool_calls,
            usage,
            duration_ms,
        })
    }
}
