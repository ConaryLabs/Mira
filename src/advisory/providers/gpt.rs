//! GPT-5.2 Provider with tool calling support

#![allow(dead_code)]

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{
    AdvisoryCapabilities, AdvisoryEvent, AdvisoryModel,
    AdvisoryProvider, AdvisoryRequest, AdvisoryResponse, AdvisoryRole,
    AdvisoryUsage, ToolCallRequest, get_env_var, DEFAULT_TIMEOUT_SECS,
};
use crate::advisory::tool_bridge;

const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";

pub struct GptProvider {
    client: Client,
    api_key: String,
    capabilities: AdvisoryCapabilities,
}

impl GptProvider {
    pub fn from_env() -> Result<Self> {
        let api_key = get_env_var("OPENAI_API_KEY")
            .ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

        Ok(Self {
            client: Client::new(),
            api_key,
            capabilities: AdvisoryCapabilities {
                supports_streaming: true,
                supports_reasoning: true,
                supports_tools: true,
                max_context_tokens: 400_000,
                max_output_tokens: 32_000,
            },
        })
    }
}

// ============================================================================
// API Types
// ============================================================================

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_completion_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
}

#[derive(Serialize, Clone)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunction,
}

#[derive(Serialize, Deserialize, Clone)]
struct OpenAIFunction {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Option<Vec<OpenAIChoice>>,
    error: Option<OpenAIError>,
    usage: Option<OpenAIUsage>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessageResponse,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIMessageResponse {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Deserialize)]
struct OpenAIError {
    message: String,
}

#[derive(Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

// ============================================================================
// Provider Implementation
// ============================================================================

#[async_trait]
impl AdvisoryProvider for GptProvider {
    fn name(&self) -> &'static str {
        "GPT-5.2"
    }

    fn model(&self) -> AdvisoryModel {
        AdvisoryModel::Gpt52
    }

    fn capabilities(&self) -> &AdvisoryCapabilities {
        &self.capabilities
    }

    async fn complete(&self, request: AdvisoryRequest) -> Result<AdvisoryResponse> {
        let mut messages = vec![];

        // Add system message if provided
        if let Some(system) = &request.system {
            messages.push(OpenAIMessage {
                role: "system".to_string(),
                content: Some(system.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Add history
        for msg in &request.history {
            messages.push(OpenAIMessage {
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
        messages.push(OpenAIMessage {
            role: "user".to_string(),
            content: Some(request.message.clone()),
            tool_calls: None,
            tool_call_id: None,
        });

        // Build tools list if enabled
        let tools = if request.enable_tools {
            Some(tool_bridge::all_openai_schemas())
        } else {
            None
        };

        let api_request = OpenAIRequest {
            model: "gpt-5.2".to_string(),
            messages,
            max_completion_tokens: 32000,
            reasoning_effort: Some("high".to_string()),
            stream: None,
            tools,
        };

        let response = self.client
            .post(OPENAI_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error: {} - {}", status, body);
        }

        let api_response: OpenAIResponse = response.json().await?;

        if let Some(error) = api_response.error {
            anyhow::bail!("OpenAI error: {}", error.message);
        }

        let choice = api_response.choices
            .and_then(|c| c.into_iter().next());

        let text = choice.as_ref()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        // Extract tool calls if present
        let tool_calls = choice.as_ref()
            .and_then(|c| c.message.tool_calls.as_ref())
            .map(|calls| {
                calls.iter().map(|tc| {
                    // Parse arguments JSON string into Value
                    let args: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(Value::Object(serde_json::Map::new()));
                    ToolCallRequest {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: args,
                    }
                }).collect()
            })
            .unwrap_or_default();

        let usage = api_response.usage.map(|u| AdvisoryUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            reasoning_tokens: 0,
        });

        Ok(AdvisoryResponse {
            text,
            usage,
            model: AdvisoryModel::Gpt52,
            tool_calls,
        })
    }

    async fn stream(
        &self,
        request: AdvisoryRequest,
        tx: mpsc::Sender<AdvisoryEvent>,
    ) -> Result<String> {
        let mut messages = vec![];

        if let Some(system) = &request.system {
            messages.push(OpenAIMessage {
                role: "system".to_string(),
                content: Some(system.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        for msg in &request.history {
            messages.push(OpenAIMessage {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: Some(msg.content.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        messages.push(OpenAIMessage {
            role: "user".to_string(),
            content: Some(request.message.clone()),
            tool_calls: None,
            tool_call_id: None,
        });

        // Note: Streaming with tools is more complex - for now, tools only work with complete()
        let api_request = OpenAIRequest {
            model: "gpt-5.2".to_string(),
            messages,
            max_completion_tokens: 32000,
            reasoning_effort: Some("high".to_string()),
            stream: Some(true),
            tools: None, // Tools not supported in streaming mode yet
        };

        let response = self.client
            .post(OPENAI_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error: {} - {}", status, body);
        }

        parse_openai_sse(response, tx).await
    }
}

// ============================================================================
// SSE Parsing
// ============================================================================

async fn parse_openai_sse(
    response: reqwest::Response,
    tx: mpsc::Sender<AdvisoryEvent>,
) -> Result<String> {
    let mut full_text = String::new();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

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
                #[derive(Deserialize)]
                struct StreamChunk {
                    choices: Option<Vec<StreamChoice>>,
                }
                #[derive(Deserialize)]
                struct StreamChoice {
                    delta: Option<StreamDelta>,
                }
                #[derive(Deserialize)]
                struct StreamDelta {
                    content: Option<String>,
                }

                if let Ok(chunk) = serde_json::from_str::<StreamChunk>(json_str) {
                    if let Some(choices) = chunk.choices {
                        for choice in choices {
                            if let Some(delta) = choice.delta {
                                if let Some(content) = delta.content {
                                    full_text.push_str(&content);
                                    let _ = tx.send(AdvisoryEvent::TextDelta(content)).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let _ = tx.send(AdvisoryEvent::Done).await;
    Ok(full_text)
}
