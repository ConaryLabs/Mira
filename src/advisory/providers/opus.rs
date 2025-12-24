//! Opus 4.5 Provider (Anthropic)

#![allow(dead_code)]

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

use super::{
    AdvisoryCapabilities, AdvisoryEvent, AdvisoryModel,
    AdvisoryProvider, AdvisoryRequest, AdvisoryResponse, AdvisoryRole,
    AdvisoryUsage, get_env_var, DEFAULT_TIMEOUT_SECS,
};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";

pub struct OpusProvider {
    client: Client,
    api_key: String,
    capabilities: AdvisoryCapabilities,
}

impl OpusProvider {
    pub fn from_env() -> Result<Self> {
        let api_key = get_env_var("ANTHROPIC_API_KEY")
            .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;

        Ok(Self {
            client: Client::new(),
            api_key,
            capabilities: AdvisoryCapabilities {
                supports_streaming: true,
                supports_reasoning: true, // Extended thinking
                supports_tools: false,    // Not implemented yet
                max_context_tokens: 200_000,
                max_output_tokens: 64_000,
            },
        })
    }
}

// ============================================================================
// API Types
// ============================================================================

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    thinking_type: String,
    budget_tokens: u32,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Option<Vec<AnthropicContent>>,
    error: Option<AnthropicError>,
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicError {
    message: String,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

// ============================================================================
// Provider Implementation
// ============================================================================

#[async_trait]
impl AdvisoryProvider for OpusProvider {
    fn name(&self) -> &'static str {
        "Opus 4.5"
    }

    fn model(&self) -> AdvisoryModel {
        AdvisoryModel::Opus45
    }

    fn capabilities(&self) -> &AdvisoryCapabilities {
        &self.capabilities
    }

    async fn complete(&self, request: AdvisoryRequest) -> Result<AdvisoryResponse> {
        let mut messages = vec![];

        // Add history
        for msg in &request.history {
            messages.push(AnthropicMessage {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: msg.content.clone(),
            });
        }

        // Add current message
        messages.push(AnthropicMessage {
            role: "user".to_string(),
            content: request.message,
        });

        let api_request = AnthropicRequest {
            model: "claude-opus-4-5-20251101".to_string(),
            max_tokens: 64000,
            messages,
            system: request.system,
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".to_string(),
                budget_tokens: 32000,
            }),
            stream: None,
        };

        let response = self.client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&api_request)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error: {} - {}", status, body);
        }

        let api_response: AnthropicResponse = response.json().await?;

        if let Some(error) = api_response.error {
            anyhow::bail!("Anthropic error: {}", error.message);
        }

        // Extract text from content blocks (skip thinking blocks)
        let text = api_response
            .content
            .map(|contents| {
                contents
                    .into_iter()
                    .filter(|c| c.content_type.as_deref() == Some("text"))
                    .filter_map(|c| c.text)
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let usage = api_response.usage.map(|u| AdvisoryUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            reasoning_tokens: 0, // Anthropic doesn't separate thinking tokens
        });

        Ok(AdvisoryResponse {
            text,
            usage,
            model: AdvisoryModel::Opus45,
            tool_calls: vec![],
        })
    }

    async fn stream(
        &self,
        request: AdvisoryRequest,
        tx: mpsc::Sender<AdvisoryEvent>,
    ) -> Result<String> {
        let mut messages = vec![];

        for msg in &request.history {
            messages.push(AnthropicMessage {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: msg.content.clone(),
            });
        }

        messages.push(AnthropicMessage {
            role: "user".to_string(),
            content: request.message,
        });

        let api_request = AnthropicRequest {
            model: "claude-opus-4-5-20251101".to_string(),
            max_tokens: 64000,
            messages,
            system: request.system,
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".to_string(),
                budget_tokens: 32000,
            }),
            stream: Some(true),
        };

        let response = self.client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&api_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error: {} - {}", status, body);
        }

        parse_anthropic_sse(response, tx).await
    }
}

// ============================================================================
// SSE Parsing
// ============================================================================

async fn parse_anthropic_sse(
    response: reqwest::Response,
    tx: mpsc::Sender<AdvisoryEvent>,
) -> Result<String> {
    let mut full_text = String::new();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut in_text_block = false;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(json_str) = line.strip_prefix("data: ") {
                #[derive(Deserialize)]
                struct StreamEvent {
                    #[serde(rename = "type")]
                    event_type: String,
                    delta: Option<StreamDelta>,
                    content_block: Option<ContentBlock>,
                }
                #[derive(Deserialize)]
                struct StreamDelta {
                    #[serde(rename = "type")]
                    delta_type: Option<String>,
                    text: Option<String>,
                }
                #[derive(Deserialize)]
                struct ContentBlock {
                    #[serde(rename = "type")]
                    block_type: Option<String>,
                }

                if let Ok(event) = serde_json::from_str::<StreamEvent>(json_str) {
                    match event.event_type.as_str() {
                        "content_block_start" => {
                            if let Some(block) = event.content_block {
                                in_text_block = block.block_type.as_deref() == Some("text");
                            }
                        }
                        "content_block_delta" => {
                            if in_text_block {
                                if let Some(delta) = event.delta {
                                    if let Some(text) = delta.text {
                                        full_text.push_str(&text);
                                        let _ = tx.send(AdvisoryEvent::TextDelta(text)).await;
                                    }
                                }
                            }
                        }
                        "content_block_stop" => {
                            in_text_block = false;
                        }
                        "message_stop" => {
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = tx.send(AdvisoryEvent::Done).await;
    Ok(full_text)
}
