// crates/mira-server/src/llm/deepseek/client.rs
// DeepSeek API client (non-streaming, uses deepseek-reasoner)

use super::types::{ChatResult, FunctionCall, Message, Tool, ToolCall, Usage};
use crate::llm::provider::{LlmClient, Provider};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::{debug, info, instrument, Span};
use uuid::Uuid;

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/chat/completions";

/// Request timeout - reasoner can take a while for complex queries
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Chat completion request
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>, // "auto" | "required" | "none"
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// Non-streaming chat response
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ResponseChoice>,
    usage: Option<Usage>,
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
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: ResponseFunction,
}

#[derive(Debug, Deserialize)]
struct ResponseFunction {
    name: String,
    arguments: String,
}

/// DeepSeek API client
pub struct DeepSeekClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl DeepSeekClient {
    /// Create a new DeepSeek client with appropriate timeouts
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, "deepseek-reasoner".into())
    }

    /// Create a new DeepSeek client with custom model
    pub fn with_model(api_key: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { api_key, model, client }
    }

    /// Chat using deepseek-reasoner model (non-streaming)
    #[instrument(skip(self, messages, tools), fields(request_id, model = %self.model, message_count = messages.len()))]
    pub async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        Span::current().record("request_id", &request_id);

        info!(
            request_id = %request_id,
            message_count = messages.len(),
            tool_count = tools.as_ref().map(|t| t.len()).unwrap_or(0),
            model = %self.model,
            "Starting DeepSeek chat request"
        );

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            tools,
            tool_choice: Some("auto".into()),
            max_tokens: Some(8192),
        };

        let body = serde_json::to_string(&request)?;
        debug!(request_id = %request_id, "DeepSeek request: {}", body);

        let mut attempts = 0;
        let max_attempts = 3;
        let mut backoff = Duration::from_secs(1);

        loop {
            let response_result = self
                .client
                .post(DEEPSEEK_API_URL)
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
                                "Transient error from DeepSeek, retrying in {:?}...",
                                backoff
                            );
                            tokio::time::sleep(backoff).await;
                            attempts += 1;
                            backoff *= 2;
                            continue;
                        }

                        return Err(anyhow!("DeepSeek API error {}: {}", status, error_body));
                    }

                    let data: ChatResponse = response
                        .json()
                        .await
                        .map_err(|e| anyhow!("Failed to parse DeepSeek response: {}", e))?;

                    let duration_ms = start_time.elapsed().as_millis() as u64;

                    // Extract response from first choice
                    let choice = data.choices.into_iter().next();
                    let (content, reasoning_content, tool_calls) = match choice {
                        Some(c) => {
                            let msg = c.message;
                            let tc: Option<Vec<ToolCall>> = msg.tool_calls.map(|calls| {
                                calls
                                    .into_iter()
                                    .map(|tc| ToolCall {
                                        id: tc.id,
                                        item_id: None,
                                        call_type: tc.call_type,
                                        function: FunctionCall {
                                            name: tc.function.name,
                                            arguments: tc.function.arguments,
                                        },
                                    })
                                    .collect()
                            });
                            (msg.content, msg.reasoning_content, tc)
                        }
                        None => (None, None, None),
                    };

                    // Log usage stats
                    if let Some(ref u) = data.usage {
                        info!(
                            request_id = %request_id,
                            prompt_tokens = u.prompt_tokens,
                            completion_tokens = u.completion_tokens,
                            cache_hit = ?u.prompt_cache_hit_tokens,
                            cache_miss = ?u.prompt_cache_miss_tokens,
                            "DeepSeek usage stats"
                        );
                    }

                    // Log tool calls if any
                    if let Some(ref tcs) = tool_calls {
                        info!(
                            request_id = %request_id,
                            tool_count = tcs.len(),
                            tools = ?tcs.iter().map(|tc| &tc.function.name).collect::<Vec<_>>(),
                            "DeepSeek requested tool calls"
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
                        "DeepSeek chat complete"
                    );

                    return Ok(ChatResult {
                        request_id,
                        content,
                        reasoning_content,
                        tool_calls,
                        usage: data.usage,
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
                    return Err(anyhow!("DeepSeek request failed after retries: {}", e));
                }
            }
        }
    }
}

#[async_trait]
impl LlmClient for DeepSeekClient {
    fn provider_type(&self) -> Provider {
        Provider::DeepSeek
    }

    fn model_name(&self) -> String {
        self.model.clone()
    }

    async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        // Delegate to the existing implementation
        self.chat(messages, tools).await
    }
}
