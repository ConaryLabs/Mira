// crates/mira-server/src/llm/openai/client.rs
// OpenAI API client (non-streaming, supports tool calling)

use crate::llm::deepseek::{ChatResult, FunctionCall, Message, Tool, ToolCall, Usage};
use crate::llm::provider::{LlmClient, Provider};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::{debug, info, instrument, Span};
use uuid::Uuid;

const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";

/// Request timeout - allow time for complex reasoning
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default model
const DEFAULT_MODEL: &str = "gpt-5.2";

/// Chat completion request (OpenAI format)
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// OpenAI message format
#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

/// OpenAI tool call format
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAiFunction,
}

/// OpenAI function format
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiFunction {
    name: String,
    arguments: String,
}

/// OpenAI tool definition
#[derive(Debug, Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiFunctionDef,
}

/// OpenAI function definition
#[derive(Debug, Serialize)]
struct OpenAiFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

/// Chat response
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ResponseChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct ResponseChoice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// OpenAI API client
pub struct OpenAiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
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
        }
    }

    /// Convert internal Message to OpenAI format
    fn convert_message(msg: &Message) -> OpenAiMessage {
        OpenAiMessage {
            role: msg.role.clone(),
            content: msg.content.clone(),
            tool_calls: msg.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| OpenAiToolCall {
                        id: tc.id.clone(),
                        call_type: tc.call_type.clone(),
                        function: OpenAiFunction {
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.clone(),
                        },
                    })
                    .collect()
            }),
            tool_call_id: msg.tool_call_id.clone(),
        }
    }

    /// Convert internal Tool to OpenAI format
    fn convert_tool(tool: &Tool) -> OpenAiTool {
        OpenAiTool {
            tool_type: tool.tool_type.clone(),
            function: OpenAiFunctionDef {
                name: tool.function.name.clone(),
                description: tool.function.description.clone(),
                parameters: tool.function.parameters.clone(),
            },
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
            "Starting OpenAI chat request"
        );

        // Convert messages and tools to OpenAI format
        let openai_messages: Vec<OpenAiMessage> = messages.iter().map(Self::convert_message).collect();
        let openai_tools: Option<Vec<OpenAiTool>> =
            tools.as_ref().map(|t| t.iter().map(Self::convert_tool).collect());

        let request = ChatRequest {
            model: self.model.clone(),
            messages: openai_messages,
            tools: openai_tools,
            tool_choice: if tools.is_some() {
                Some("auto".into())
            } else {
                None
            },
            max_tokens: Some(8192),
        };

        debug!(request_id = %request_id, "OpenAI request: {:?}", serde_json::to_string(&request)?);

        let response = self
            .client
            .post(OPENAI_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow!("OpenAI request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI API error {}: {}", status, body));
        }

        let data: ChatResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse OpenAI response: {}", e))?;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Extract response from first choice
        let choice = data.choices.into_iter().next();
        let (content, tool_calls) = match choice {
            Some(c) => {
                let msg = c.message;
                let tc: Option<Vec<ToolCall>> = msg.tool_calls.map(|calls| {
                    calls
                        .into_iter()
                        .map(|tc| ToolCall {
                            id: tc.id,
                            call_type: tc.call_type,
                            function: FunctionCall {
                                name: tc.function.name,
                                arguments: tc.function.arguments,
                            },
                        })
                        .collect()
                });
                (msg.content, tc)
            }
            None => (None, None),
        };

        // Convert usage
        let usage = data.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
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
                "OpenAI usage stats"
            );
        }

        // Log tool calls if any
        if let Some(ref tcs) = tool_calls {
            info!(
                request_id = %request_id,
                tool_count = tcs.len(),
                tools = ?tcs.iter().map(|tc| &tc.function.name).collect::<Vec<_>>(),
                "OpenAI requested tool calls"
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
            tool_calls = tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
            "OpenAI chat complete"
        );

        Ok(ChatResult {
            request_id,
            content,
            reasoning_content: None, // OpenAI doesn't have reasoning_content
            tool_calls,
            usage,
            duration_ms,
        })
    }
}
