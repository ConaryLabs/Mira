//! Gemini 3 Pro Provider

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

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-preview:generateContent";

pub struct GeminiProvider {
    client: Client,
    api_key: String,
    capabilities: AdvisoryCapabilities,
}

impl GeminiProvider {
    pub fn from_env() -> Result<Self> {
        let api_key = get_env_var("GEMINI_API_KEY")
            .ok_or_else(|| anyhow::anyhow!("GEMINI_API_KEY not set"))?;

        Ok(Self {
            client: Client::new(),
            api_key,
            capabilities: AdvisoryCapabilities {
                supports_streaming: true,
                supports_reasoning: true,
                supports_tools: false, // Not implemented yet
                max_context_tokens: 1_000_000, // Gemini has huge context
                max_output_tokens: 65_536,
            },
        })
    }
}

// ============================================================================
// API Types
// ============================================================================

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "thinkingConfig")]
    thinking_config: GeminiThinkingConfig,
}

#[derive(Serialize)]
struct GeminiThinkingConfig {
    #[serde(rename = "thinkingLevel")]
    thinking_level: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
    error: Option<GeminiError>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContentResponse,
}

#[derive(Deserialize)]
struct GeminiContentResponse {
    parts: Vec<GeminiPartResponse>,
}

#[derive(Deserialize)]
struct GeminiPartResponse {
    text: Option<String>,
}

#[derive(Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
}

#[derive(Deserialize)]
struct GeminiError {
    message: String,
}

// ============================================================================
// Provider Implementation
// ============================================================================

#[async_trait]
impl AdvisoryProvider for GeminiProvider {
    fn name(&self) -> &'static str {
        "Gemini 3 Pro"
    }

    fn model(&self) -> AdvisoryModel {
        AdvisoryModel::Gemini3Pro
    }

    fn capabilities(&self) -> &AdvisoryCapabilities {
        &self.capabilities
    }

    async fn complete(&self, request: AdvisoryRequest) -> Result<AdvisoryResponse> {
        let mut contents = vec![];

        // Add history
        for msg in &request.history {
            contents.push(GeminiContent {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "model".to_string(),
                },
                parts: vec![GeminiPart { text: msg.content.clone() }],
            });
        }

        // Add current message
        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart { text: request.message }],
        });

        let api_request = GeminiRequest {
            contents,
            system_instruction: request.system.map(|s| GeminiSystemInstruction {
                parts: vec![GeminiPart { text: s }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: "high".to_string(),
                },
            }),
        };

        let url = format!("{}?key={}", GEMINI_API_URL, self.api_key);

        let response = self.client
            .post(&url)
            .json(&api_request)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error: {} - {}", status, body);
        }

        let api_response: GeminiResponse = response.json().await?;

        if let Some(error) = api_response.error {
            anyhow::bail!("Gemini error: {}", error.message);
        }

        let text = api_response
            .candidates
            .and_then(|c| c.into_iter().next())
            .map(|c| {
                c.content
                    .parts
                    .into_iter()
                    .filter_map(|p| p.text)
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let usage = api_response.usage_metadata.map(|u| AdvisoryUsage {
            input_tokens: u.prompt_token_count.unwrap_or(0),
            output_tokens: u.candidates_token_count.unwrap_or(0),
            reasoning_tokens: 0,
        });

        Ok(AdvisoryResponse {
            text,
            usage,
            model: AdvisoryModel::Gemini3Pro,
            tool_calls: vec![],
        })
    }

    async fn stream(
        &self,
        request: AdvisoryRequest,
        tx: mpsc::Sender<AdvisoryEvent>,
    ) -> Result<String> {
        let mut contents = vec![];

        for msg in &request.history {
            contents.push(GeminiContent {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "model".to_string(),
                },
                parts: vec![GeminiPart { text: msg.content.clone() }],
            });
        }

        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart { text: request.message }],
        });

        let api_request = GeminiRequest {
            contents,
            system_instruction: request.system.map(|s| GeminiSystemInstruction {
                parts: vec![GeminiPart { text: s }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: "high".to_string(),
                },
            }),
        };

        // Use streaming endpoint
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-preview:streamGenerateContent?key={}&alt=sse",
            self.api_key
        );

        let response = self.client
            .post(&url)
            .json(&api_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error: {} - {}", status, body);
        }

        parse_gemini_sse(response, tx).await
    }
}

// ============================================================================
// SSE Parsing
// ============================================================================

async fn parse_gemini_sse(
    response: reqwest::Response,
    tx: mpsc::Sender<AdvisoryEvent>,
) -> Result<String> {
    let mut full_text = String::new();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process SSE data lines
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(json_str) = line.strip_prefix("data: ") {
                #[derive(Deserialize)]
                struct StreamChunk {
                    candidates: Option<Vec<StreamCandidate>>,
                }
                #[derive(Deserialize)]
                struct StreamCandidate {
                    content: Option<StreamContent>,
                }
                #[derive(Deserialize)]
                struct StreamContent {
                    parts: Option<Vec<StreamPart>>,
                }
                #[derive(Deserialize)]
                struct StreamPart {
                    text: Option<String>,
                }

                if let Ok(chunk) = serde_json::from_str::<StreamChunk>(json_str) {
                    if let Some(candidates) = chunk.candidates {
                        for candidate in candidates {
                            if let Some(content) = candidate.content {
                                if let Some(parts) = content.parts {
                                    for part in parts {
                                        if let Some(text) = part.text {
                                            full_text.push_str(&text);
                                            let _ = tx.send(AdvisoryEvent::TextDelta(text)).await;
                                        }
                                    }
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
