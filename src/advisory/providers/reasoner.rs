//! DeepSeek Reasoner Provider (with Tool Calling)
//!
//! Uses the same Chat Completions API as the chat provider but adapted
//! for the advisory system's simpler request/response types.
//!
//! Includes exponential backoff with jitter for rate limiting resilience.

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{
    AdvisoryCapabilities, AdvisoryEvent, AdvisoryModel,
    AdvisoryProvider, AdvisoryRequest, AdvisoryResponse, AdvisoryRole,
    AdvisoryUsage, ToolCallRequest, get_env_var, REASONER_TIMEOUT_SECS,
};
use crate::advisory::tool_bridge::{AllowedTool, chat_completions_tool_schema};

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/v1/chat/completions";

/// Maximum number of retry attempts for rate-limited requests
const MAX_RETRIES: u32 = 3;

/// Initial backoff delay in milliseconds
const INITIAL_BACKOFF_MS: u64 = 1000;

/// Maximum backoff delay in milliseconds (30 seconds)
const MAX_BACKOFF_MS: u64 = 30_000;

/// Calculate backoff with exponential increase and jitter
fn backoff_with_jitter(attempt: u32) -> Duration {
    let base_delay = INITIAL_BACKOFF_MS * 2u64.pow(attempt);
    let capped_delay = base_delay.min(MAX_BACKOFF_MS);
    // Add 0-50% jitter to prevent thundering herd
    let jitter = rand::rng().random_range(0..=(capped_delay / 2));
    Duration::from_millis(capped_delay + jitter)
}

/// Check if an error is retryable (rate limit or temporary server error)
fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        || status == reqwest::StatusCode::GATEWAY_TIMEOUT
        || status == reqwest::StatusCode::BAD_GATEWAY
}

pub struct ReasonerProvider {
    client: Client,
    api_key: String,
    capabilities: AdvisoryCapabilities,
}

impl ReasonerProvider {
    pub fn from_env() -> Result<Self> {
        let api_key = get_env_var("DEEPSEEK_API_KEY")
            .ok_or_else(|| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;

        Ok(Self {
            client: Client::new(),
            api_key,
            capabilities: AdvisoryCapabilities {
                supports_streaming: true,
                supports_reasoning: true,
                supports_tools: true,
                max_context_tokens: 128_000,
                max_output_tokens: 64_000,
            },
        })
    }

    /// Build tool definitions for the API
    fn build_tools() -> Vec<ChatTool> {
        AllowedTool::all()
            .iter()
            .map(|tool| {
                let schema = chat_completions_tool_schema(*tool);
                ChatTool {
                    tool_type: "function".to_string(),
                    function: ChatFunction {
                        name: schema["function"]["name"].as_str().unwrap_or("").to_string(),
                        description: schema["function"]["description"].as_str().map(String::from),
                        parameters: schema["function"]["parameters"].clone(),
                    },
                }
            })
            .collect()
    }
}

// ============================================================================
// API Types (OpenAI-compatible Chat Completions format)
// ============================================================================

#[derive(Serialize)]
struct DeepSeekRequest {
    model: String,
    messages: Vec<DeepSeekMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
}

#[derive(Serialize)]
struct DeepSeekMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ChatToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: ChatFunction,
}

#[derive(Debug, Serialize)]
struct ChatFunction {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: ChatToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct DeepSeekResponse {
    choices: Option<Vec<DeepSeekChoice>>,
    error: Option<DeepSeekError>,
    usage: Option<DeepSeekUsage>,
}

#[derive(Deserialize)]
struct DeepSeekChoice {
    message: DeepSeekMessageResponse,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct DeepSeekMessageResponse {
    content: Option<String>,
    tool_calls: Option<Vec<ChatToolCall>>,
}

#[derive(Deserialize)]
struct DeepSeekError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct DeepSeekUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    #[serde(default)]
    reasoning_tokens: u32,
    #[serde(default)]
    prompt_cache_hit_tokens: u32,
    #[serde(default)]
    prompt_cache_miss_tokens: u32,
}

// Streaming types
#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
    usage: Option<DeepSeekUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<StreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    #[serde(default)]
    index: usize,
    id: Option<String>,
    function: Option<StreamFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

// ============================================================================
// Provider Implementation
// ============================================================================

#[async_trait]
impl AdvisoryProvider for ReasonerProvider {
    fn name(&self) -> &'static str {
        "DeepSeek Reasoner"
    }

    fn model(&self) -> AdvisoryModel {
        AdvisoryModel::DeepSeekReasoner
    }

    fn capabilities(&self) -> &AdvisoryCapabilities {
        &self.capabilities
    }

    async fn complete(&self, request: AdvisoryRequest) -> Result<AdvisoryResponse> {
        let mut messages = vec![];

        // Add system message if provided
        if let Some(system) = &request.system {
            messages.push(DeepSeekMessage {
                role: "system".to_string(),
                content: Some(system.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Add history
        for msg in &request.history {
            messages.push(DeepSeekMessage {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: Some(msg.content.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Add current message
        messages.push(DeepSeekMessage {
            role: "user".to_string(),
            content: Some(request.message),
            tool_calls: None,
            tool_call_id: None,
        });

        let tools = if request.enable_tools {
            Some(Self::build_tools())
        } else {
            None
        };

        let api_request = DeepSeekRequest {
            model: "deepseek-reasoner".to_string(),
            messages,
            max_tokens: 8192,
            stream: None,
            tools,
        };

        // Retry loop with exponential backoff
        let mut last_error = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = backoff_with_jitter(attempt - 1);
                tracing::warn!(
                    attempt = attempt,
                    backoff_ms = backoff.as_millis(),
                    "DeepSeek Reasoner: retrying after backoff"
                );
                tokio::time::sleep(backoff).await;
            }

            let response = match self.client
                .post(DEEPSEEK_API_URL)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&api_request)
                .timeout(Duration::from_secs(REASONER_TIMEOUT_SECS))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    // Network error - retry if attempts remain
                    last_error = Some(format!("Network error: {}", e));
                    if attempt < MAX_RETRIES {
                        continue;
                    }
                    anyhow::bail!("DeepSeek Reasoner network error after {} retries: {}", MAX_RETRIES, e);
                }
            };

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();

                if is_retryable_status(status) && attempt < MAX_RETRIES {
                    tracing::warn!(
                        status = %status,
                        attempt = attempt,
                        "DeepSeek Reasoner: retryable error"
                    );
                    last_error = Some(format!("{} - {}", status, body));
                    continue;
                }

                anyhow::bail!("DeepSeek Reasoner API error: {} - {}", status, body);
            }

            // Success - parse response
            let api_response: DeepSeekResponse = response.json().await?;

            if let Some(error) = api_response.error {
                anyhow::bail!("DeepSeek Reasoner error: {}", error.message);
            }

            // Extract response content
            let choice = api_response.choices
                .and_then(|c| c.into_iter().next());

            let text = choice.as_ref()
                .and_then(|c| c.message.content.clone())
                .unwrap_or_default();

            let tool_calls = choice
                .and_then(|c| c.message.tool_calls)
                .map(|tcs| {
                    tcs.into_iter()
                        .filter_map(|tc| {
                            let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                            Some(ToolCallRequest {
                                id: tc.id,
                                name: tc.function.name,
                                arguments: args,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            let usage = api_response.usage.map(|u| AdvisoryUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
                reasoning_tokens: u.reasoning_tokens,
                cache_read_tokens: u.prompt_cache_hit_tokens,
                cache_write_tokens: u.prompt_cache_miss_tokens,
            });

            return Ok(AdvisoryResponse {
                text,
                usage,
                model: AdvisoryModel::DeepSeekReasoner,
                tool_calls,
                reasoning: None,
            });
        }

        // Should not reach here, but just in case
        anyhow::bail!("DeepSeek Reasoner failed after {} retries: {:?}", MAX_RETRIES, last_error)
    }

    async fn stream(
        &self,
        request: AdvisoryRequest,
        tx: mpsc::Sender<AdvisoryEvent>,
    ) -> Result<String> {
        let mut messages = vec![];

        if let Some(system) = &request.system {
            messages.push(DeepSeekMessage {
                role: "system".to_string(),
                content: Some(system.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        for msg in &request.history {
            messages.push(DeepSeekMessage {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: Some(msg.content.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        messages.push(DeepSeekMessage {
            role: "user".to_string(),
            content: Some(request.message),
            tool_calls: None,
            tool_call_id: None,
        });

        let tools = if request.enable_tools {
            Some(Self::build_tools())
        } else {
            None
        };

        let api_request = DeepSeekRequest {
            model: "deepseek-reasoner".to_string(),
            messages,
            max_tokens: 8192,
            stream: Some(true),
            tools,
        };

        let response = self.client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("DeepSeek Reasoner API error: {} - {}", status, body);
        }

        parse_deepseek_sse(response, tx).await
    }
}

// ============================================================================
// SSE Parsing with Tool Call Support
// ============================================================================

/// Track in-flight tool calls during streaming
struct InFlightCall {
    id: String,
    name: String,
    args: String,
}

async fn parse_deepseek_sse(
    response: reqwest::Response,
    tx: mpsc::Sender<AdvisoryEvent>,
) -> Result<String> {
    let mut full_text = String::new();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut tool_calls: HashMap<usize, InFlightCall> = HashMap::new();
    let mut collected_tool_calls: Vec<ToolCallRequest> = vec![];

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() || line == "data: [DONE]" {
                continue;
            }

            if let Some(json_str) = line.strip_prefix("data: ") {
                if let Ok(chunk) = serde_json::from_str::<StreamChunk>(json_str) {
                    if let Some(choices) = chunk.choices {
                        for choice in choices {
                            if let Some(delta) = choice.delta {
                                // Handle reasoning content
                                if let Some(reasoning) = delta.reasoning_content {
                                    if !reasoning.is_empty() {
                                        let _ = tx.send(AdvisoryEvent::ReasoningDelta(reasoning)).await;
                                    }
                                }

                                // Handle text content
                                if let Some(content) = delta.content {
                                    if !content.is_empty() {
                                        full_text.push_str(&content);
                                        let _ = tx.send(AdvisoryEvent::TextDelta(content)).await;
                                    }
                                }

                                // Handle tool calls - track by index for parallel calls
                                if let Some(delta_tool_calls) = delta.tool_calls {
                                    for tc in delta_tool_calls {
                                        let idx = tc.index;

                                        let call = tool_calls.entry(idx).or_insert_with(|| InFlightCall {
                                            id: String::new(),
                                            name: String::new(),
                                            args: String::new(),
                                        });

                                        // Update ID if present
                                        if let Some(ref id) = tc.id {
                                            call.id = id.clone();
                                        }

                                        // Update name if present
                                        if let Some(ref func) = tc.function {
                                            if let Some(ref name) = func.name {
                                                call.name = name.clone();
                                            }
                                            // Accumulate arguments
                                            if let Some(ref args) = func.arguments {
                                                call.args.push_str(args);
                                            }
                                        }
                                    }
                                }
                            }

                            // On finish, collect all tool calls
                            if choice.finish_reason.is_some() {
                                for (_, call) in tool_calls.drain() {
                                    if !call.id.is_empty() && !call.name.is_empty() {
                                        let args: serde_json::Value = serde_json::from_str(&call.args)
                                            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                                        collected_tool_calls.push(ToolCallRequest {
                                            id: call.id,
                                            name: call.name,
                                            arguments: args,
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // Usage info
                    if let Some(usage) = chunk.usage {
                        let _ = tx.send(AdvisoryEvent::Usage(AdvisoryUsage {
                            input_tokens: usage.prompt_tokens,
                            output_tokens: usage.completion_tokens,
                            reasoning_tokens: usage.reasoning_tokens,
                            cache_read_tokens: usage.prompt_cache_hit_tokens,
                            cache_write_tokens: usage.prompt_cache_miss_tokens,
                        })).await;
                    }
                }
            }
        }
    }

    let _ = tx.send(AdvisoryEvent::Done).await;

    // If we have tool calls but no text, return a marker
    // The caller will need to handle tool execution
    if !collected_tool_calls.is_empty() && full_text.is_empty() {
        // Encode tool calls in the response for the caller to parse
        // This is a workaround since stream() returns String, not AdvisoryResponse
        full_text = format!("[TOOL_CALLS]{}", serde_json::to_string(&collected_tool_calls)?);
    }

    Ok(full_text)
}
