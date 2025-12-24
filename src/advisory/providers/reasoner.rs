//! DeepSeek Reasoner Provider (Synthesizer)

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
    AdvisoryUsage, get_env_var, REASONER_TIMEOUT_SECS,
};

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/v1/chat/completions";

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
                supports_tools: false, // Reasoner doesn't support tools
                max_context_tokens: 128_000,
                max_output_tokens: 64_000,
            },
        })
    }
}

// ============================================================================
// API Types
// ============================================================================

#[derive(Serialize)]
struct DeepSeekRequest {
    model: String,
    messages: Vec<DeepSeekMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct DeepSeekMessage {
    role: String,
    content: String,
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
}

#[derive(Deserialize)]
struct DeepSeekMessageResponse {
    content: Option<String>,
}

#[derive(Deserialize)]
struct DeepSeekError {
    message: String,
}

#[derive(Deserialize)]
struct DeepSeekUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    #[serde(default)]
    reasoning_tokens: u32,
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
                content: system.clone(),
            });
        }

        // Add history
        for msg in &request.history {
            messages.push(DeepSeekMessage {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: msg.content.clone(),
            });
        }

        // Add current message
        messages.push(DeepSeekMessage {
            role: "user".to_string(),
            content: request.message,
        });

        let api_request = DeepSeekRequest {
            model: "deepseek-reasoner".to_string(),
            messages,
            max_tokens: 8192,
            stream: None,
        };

        let response = self.client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .timeout(Duration::from_secs(REASONER_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("DeepSeek Reasoner API error: {} - {}", status, body);
        }

        let api_response: DeepSeekResponse = response.json().await?;

        if let Some(error) = api_response.error {
            anyhow::bail!("DeepSeek Reasoner error: {}", error.message);
        }

        let text = api_response
            .choices
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.message.content)
            .unwrap_or_default();

        let usage = api_response.usage.map(|u| AdvisoryUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            reasoning_tokens: u.reasoning_tokens,
        });

        Ok(AdvisoryResponse {
            text,
            usage,
            model: AdvisoryModel::DeepSeekReasoner,
            tool_calls: vec![],
        })
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
                content: system.clone(),
            });
        }

        for msg in &request.history {
            messages.push(DeepSeekMessage {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: msg.content.clone(),
            });
        }

        messages.push(DeepSeekMessage {
            role: "user".to_string(),
            content: request.message,
        });

        let api_request = DeepSeekRequest {
            model: "deepseek-reasoner".to_string(),
            messages,
            max_tokens: 8192,
            stream: Some(true),
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
// SSE Parsing
// ============================================================================

async fn parse_deepseek_sse(
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
                    reasoning_content: Option<String>,
                }

                if let Ok(chunk) = serde_json::from_str::<StreamChunk>(json_str) {
                    if let Some(choices) = chunk.choices {
                        for choice in choices {
                            if let Some(delta) = choice.delta {
                                // Send reasoning as separate event
                                if let Some(reasoning) = delta.reasoning_content {
                                    let _ = tx.send(AdvisoryEvent::ReasoningDelta(reasoning)).await;
                                }
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
