//! Advisory Provider trait and implementations
//!
//! Provides a unified interface for calling different LLM providers.

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

const DOTENV_PATH: &str = "/home/peter/Mira/.env";
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const REASONER_TIMEOUT_SECS: u64 = 180;

// API endpoints
const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";
const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/v1/chat/completions";
const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-preview:generateContent";

// ============================================================================
// Core Types
// ============================================================================

/// Available advisory models
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdvisoryModel {
    Gpt52,
    Opus45,
    Gemini3Pro,
    DeepSeekReasoner,
}

impl AdvisoryModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            AdvisoryModel::Gpt52 => "gpt-5.2",
            AdvisoryModel::Opus45 => "opus-4.5",
            AdvisoryModel::Gemini3Pro => "gemini-3-pro",
            AdvisoryModel::DeepSeekReasoner => "deepseek-reasoner",
        }
    }
}

/// Request to an advisory provider
#[derive(Debug, Clone)]
pub struct AdvisoryRequest {
    /// The message/question
    pub message: String,
    /// System prompt / instructions
    pub system: Option<String>,
    /// Previous conversation turns (for multi-turn)
    pub history: Vec<AdvisoryMessage>,
}

/// A message in advisory conversation history
#[derive(Debug, Clone)]
pub struct AdvisoryMessage {
    pub role: AdvisoryRole,
    pub content: String,
}

/// Role in advisory conversation
#[derive(Debug, Clone, Copy)]
pub enum AdvisoryRole {
    User,
    Assistant,
}

/// Response from an advisory provider
#[derive(Debug, Clone)]
pub struct AdvisoryResponse {
    pub text: String,
    pub usage: Option<AdvisoryUsage>,
    pub model: AdvisoryModel,
}

/// Token usage information
#[derive(Debug, Clone, Default)]
pub struct AdvisoryUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub reasoning_tokens: u32,
}

/// Streaming event from advisory provider
#[derive(Debug, Clone)]
pub enum AdvisoryEvent {
    /// Text content delta
    TextDelta(String),
    /// Reasoning/thinking delta (for reasoning models)
    ReasoningDelta(String),
    /// Usage information
    Usage(AdvisoryUsage),
    /// Stream complete
    Done,
    /// Error occurred
    Error(String),
}

/// Provider capabilities
#[derive(Debug, Clone)]
pub struct AdvisoryCapabilities {
    pub supports_streaming: bool,
    pub supports_reasoning: bool,
    pub max_context_tokens: u32,
    pub max_output_tokens: u32,
}

// ============================================================================
// Provider Trait
// ============================================================================

/// Core advisory provider trait
#[async_trait]
pub trait AdvisoryProvider: Send + Sync {
    /// Provider name for logging/identification
    fn name(&self) -> &'static str;

    /// Which model this provider represents
    fn model(&self) -> AdvisoryModel;

    /// Get provider capabilities
    fn capabilities(&self) -> &AdvisoryCapabilities;

    /// Create non-streaming response (blocks until complete)
    async fn complete(&self, request: AdvisoryRequest) -> Result<AdvisoryResponse>;

    /// Stream response with events sent to the provided channel
    /// Returns the full text when complete
    async fn stream(
        &self,
        request: AdvisoryRequest,
        tx: mpsc::Sender<AdvisoryEvent>,
    ) -> Result<String>;
}

// ============================================================================
// Environment helpers
// ============================================================================

fn get_env_var(name: &str) -> Option<String> {
    // First try env var
    if let Ok(val) = std::env::var(name) {
        return Some(val);
    }

    // Fallback: read from .env file
    if let Ok(contents) = std::fs::read_to_string(DOTENV_PATH) {
        let prefix = format!("{}=", name);
        for line in contents.lines() {
            if let Some(value) = line.strip_prefix(&prefix) {
                return Some(value.trim().to_string());
            }
        }
    }

    None
}

// ============================================================================
// GPT-5.2 Provider
// ============================================================================

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
                max_context_tokens: 400_000,
                max_output_tokens: 32_000,
            },
        })
    }
}

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_completion_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
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
}

#[derive(Deserialize)]
struct OpenAIMessageResponse {
    content: Option<String>,
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
                content: system.clone(),
            });
        }

        // Add history
        for msg in &request.history {
            messages.push(OpenAIMessage {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: msg.content.clone(),
            });
        }

        // Add current message
        messages.push(OpenAIMessage {
            role: "user".to_string(),
            content: request.message,
        });

        let api_request = OpenAIRequest {
            model: "gpt-5.2".to_string(),
            messages,
            max_completion_tokens: 32000,
            reasoning_effort: Some("high".to_string()),
            stream: None,
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

        let text = api_response
            .choices
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.message.content)
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
                content: system.clone(),
            });
        }

        for msg in &request.history {
            messages.push(OpenAIMessage {
                role: match msg.role {
                    AdvisoryRole::User => "user".to_string(),
                    AdvisoryRole::Assistant => "assistant".to_string(),
                },
                content: msg.content.clone(),
            });
        }

        messages.push(OpenAIMessage {
            role: "user".to_string(),
            content: request.message,
        });

        let api_request = OpenAIRequest {
            model: "gpt-5.2".to_string(),
            messages,
            max_completion_tokens: 32000,
            reasoning_effort: Some("high".to_string()),
            stream: Some(true),
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

/// Parse OpenAI SSE stream inline
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

// ============================================================================
// Opus 4.5 Provider
// ============================================================================

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
                max_context_tokens: 200_000,
                max_output_tokens: 64_000,
            },
        })
    }
}

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

/// Parse Anthropic SSE stream
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

// ============================================================================
// Gemini 3 Pro Provider
// ============================================================================

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
                max_context_tokens: 1_000_000, // Gemini has huge context
                max_output_tokens: 65_536,
            },
        })
    }
}

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

/// Parse Gemini SSE stream
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

// ============================================================================
// DeepSeek Reasoner Provider (Synthesizer)
// ============================================================================

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
                max_context_tokens: 128_000,
                max_output_tokens: 64_000,
            },
        })
    }
}

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

/// Parse DeepSeek SSE stream
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
