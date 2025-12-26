//! GPT-5.2 Provider using the Responses API
//!
//! Uses OpenAI's Responses API (released March 2025) which provides:
//! - Better performance (3% improvement on SWE-bench)
//! - Lower costs (40-80% better cache utilization)
//! - Native tool calling with function_call_output

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

const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

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
// Responses API Types
// ============================================================================

/// Input item for the Responses API
#[derive(Serialize, Clone, Debug)]
#[serde(untagged)]
enum ResponsesInput {
    /// Simple string input
    Text(String),
    /// Structured input with roles and types
    Items(Vec<ResponsesInputItem>),
}

/// An input item (message, function call, function call output, etc.)
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponsesInputItem {
    /// User or system message
    Message {
        role: String,
        content: String,
    },
    /// Function call from the model (must be included before output)
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    /// Function call output (tool result)
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
}

/// Request to the Responses API
#[derive(Serialize)]
struct ResponsesRequest {
    model: String,
    input: ResponsesInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct ReasoningConfig {
    effort: String,
}

/// Response from the Responses API
#[derive(Deserialize, Debug)]
struct ResponsesResponse {
    id: Option<String>,
    output: Option<Vec<ResponsesOutputItem>>,
    #[serde(default)]
    output_text: Option<String>,
    error: Option<ResponsesError>,
    usage: Option<ResponsesUsage>,
}

/// An output item from the response
#[derive(Deserialize, Debug)]
struct ResponsesOutputItem {
    #[serde(rename = "type")]
    item_type: String,
    /// For message items
    content: Option<Vec<ContentPart>>,
    /// For function_call items
    call_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ContentPart {
    #[serde(rename = "type")]
    part_type: String,
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ResponsesError {
    message: String,
    code: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ResponsesUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    reasoning_tokens: u32,
    #[serde(default)]
    input_token_details: Option<InputTokenDetails>,
}

#[derive(Deserialize, Debug, Default)]
struct InputTokenDetails {
    #[serde(default)]
    cached_tokens: u32,
}

// ============================================================================
// Provider Implementation
// ============================================================================

impl GptProvider {
    /// Complete with raw input items (for tool loop)
    pub async fn complete_with_items(
        &self,
        items: Vec<ResponsesInputItem>,
        instructions: Option<String>,
        enable_tools: bool,
    ) -> Result<AdvisoryResponse> {
        let tools = if enable_tools {
            Some(tool_bridge::all_openai_schemas())
        } else {
            None
        };

        let api_request = ResponsesRequest {
            model: "gpt-5.2".to_string(),
            input: ResponsesInput::Items(items),
            instructions,
            max_output_tokens: Some(32000),
            reasoning: Some(ReasoningConfig {
                effort: "high".to_string(),
            }),
            tools,
            stream: None,
        };

        let response = self.client
            .post(OPENAI_RESPONSES_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI Responses API error: {} - {}", status, body);
        }

        let api_response: ResponsesResponse = response.json().await?;

        if let Some(error) = api_response.error {
            anyhow::bail!("OpenAI error: {} (code: {:?})", error.message, error.code);
        }

        // Extract text and tool calls from output
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCallRequest> = vec![];

        if let Some(output) = &api_response.output {
            for item in output {
                match item.item_type.as_str() {
                    "message" => {
                        if let Some(content) = &item.content {
                            for part in content {
                                if part.part_type == "output_text" || part.part_type == "text" {
                                    if let Some(t) = &part.text {
                                        text.push_str(t);
                                    }
                                }
                            }
                        }
                    }
                    "function_call" => {
                        if let (Some(call_id), Some(name), Some(args_str)) =
                            (&item.call_id, &item.name, &item.arguments)
                        {
                            let args: Value = serde_json::from_str(args_str)
                                .unwrap_or(Value::Object(serde_json::Map::new()));
                            tool_calls.push(ToolCallRequest {
                                id: call_id.clone(),
                                name: name.clone(),
                                arguments: args,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        if text.is_empty() {
            if let Some(output_text) = api_response.output_text {
                text = output_text;
            }
        }

        let usage = api_response.usage.map(|u| AdvisoryUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            reasoning_tokens: u.reasoning_tokens,
            cache_read_tokens: u.input_token_details.map(|d| d.cached_tokens).unwrap_or(0),
            cache_write_tokens: 0, // OpenAI doesn't report cache write separately
        });

        Ok(AdvisoryResponse {
            text,
            usage,
            model: AdvisoryModel::Gpt52,
            tool_calls,
            reasoning: None, // GPT doesn't expose reasoning separately
        })
    }
}

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
        // Build input items
        let mut items: Vec<ResponsesInputItem> = vec![];

        // Add history
        for msg in &request.history {
            items.push(ResponsesInputItem::Message {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: msg.content.clone(),
            });
        }

        // Add current message
        items.push(ResponsesInputItem::Message {
            role: "user".to_string(),
            content: request.message.clone(),
        });

        // Build tools list if enabled
        let tools = if request.enable_tools {
            Some(tool_bridge::all_openai_schemas())
        } else {
            None
        };

        let api_request = ResponsesRequest {
            model: "gpt-5.2".to_string(),
            input: ResponsesInput::Items(items),
            instructions: request.system.clone(),
            max_output_tokens: Some(32000),
            reasoning: Some(ReasoningConfig {
                effort: "high".to_string(),
            }),
            tools,
            stream: None,
        };

        let response = self.client
            .post(OPENAI_RESPONSES_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI Responses API error: {} - {}", status, body);
        }

        let api_response: ResponsesResponse = response.json().await?;

        if let Some(error) = api_response.error {
            anyhow::bail!("OpenAI error: {} (code: {:?})", error.message, error.code);
        }

        // Extract text and tool calls from output
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCallRequest> = vec![];

        if let Some(output) = &api_response.output {
            for item in output {
                match item.item_type.as_str() {
                    "message" => {
                        // Extract text from content parts
                        if let Some(content) = &item.content {
                            for part in content {
                                if part.part_type == "output_text" || part.part_type == "text" {
                                    if let Some(t) = &part.text {
                                        text.push_str(t);
                                    }
                                }
                            }
                        }
                    }
                    "function_call" => {
                        // Extract function call
                        if let (Some(call_id), Some(name), Some(args_str)) =
                            (&item.call_id, &item.name, &item.arguments)
                        {
                            let args: Value = serde_json::from_str(args_str)
                                .unwrap_or(Value::Object(serde_json::Map::new()));
                            tool_calls.push(ToolCallRequest {
                                id: call_id.clone(),
                                name: name.clone(),
                                arguments: args,
                            });
                        }
                    }
                    _ => {
                        // Ignore other item types (reasoning, etc.)
                    }
                }
            }
        }

        // Fallback to output_text if available
        if text.is_empty() {
            if let Some(output_text) = api_response.output_text {
                text = output_text;
            }
        }

        let usage = api_response.usage.map(|u| AdvisoryUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            reasoning_tokens: u.reasoning_tokens,
            cache_read_tokens: u.input_token_details.map(|d| d.cached_tokens).unwrap_or(0),
            cache_write_tokens: 0,
        });

        Ok(AdvisoryResponse {
            text,
            usage,
            model: AdvisoryModel::Gpt52,
            tool_calls,
            reasoning: None,
        })
    }

    async fn stream(
        &self,
        request: AdvisoryRequest,
        tx: mpsc::Sender<AdvisoryEvent>,
    ) -> Result<String> {
        // Build input items
        let mut items: Vec<ResponsesInputItem> = vec![];

        for msg in &request.history {
            items.push(ResponsesInputItem::Message {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: msg.content.clone(),
            });
        }

        items.push(ResponsesInputItem::Message {
            role: "user".to_string(),
            content: request.message.clone(),
        });

        // Note: Streaming with tools is more complex - for now, tools only work with complete()
        let api_request = ResponsesRequest {
            model: "gpt-5.2".to_string(),
            input: ResponsesInput::Items(items),
            instructions: request.system.clone(),
            max_output_tokens: Some(32000),
            reasoning: Some(ReasoningConfig {
                effort: "high".to_string(),
            }),
            tools: None, // Tools not supported in streaming mode yet
            stream: Some(true),
        };

        let response = self.client
            .post(OPENAI_RESPONSES_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI Responses API error: {} - {}", status, body);
        }

        parse_responses_sse(response, tx).await
    }
}

// ============================================================================
// SSE Parsing for Responses API
// ============================================================================

async fn parse_responses_sse(
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
                // Responses API streaming format
                #[derive(Deserialize)]
                struct StreamEvent {
                    #[serde(rename = "type")]
                    event_type: Option<String>,
                    delta: Option<StreamDelta>,
                }
                #[derive(Deserialize)]
                struct StreamDelta {
                    text: Option<String>,
                }

                if let Ok(event) = serde_json::from_str::<StreamEvent>(json_str) {
                    if let Some(delta) = event.delta {
                        if let Some(text) = delta.text {
                            full_text.push_str(&text);
                            let _ = tx.send(AdvisoryEvent::TextDelta(text)).await;
                        }
                    }
                }
            }
        }
    }

    let _ = tx.send(AdvisoryEvent::Done).await;
    Ok(full_text)
}
