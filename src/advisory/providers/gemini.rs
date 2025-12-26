//! Gemini 3 Pro Provider with function calling support
//!
//! Uses Gemini's generateContent API with function calling.
//! Handles Gemini 3's thought signatures for multi-turn tool use.

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
                supports_tools: true,
                max_context_tokens: 1_000_000,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiTextPart>,
}

#[derive(Serialize, Clone)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

/// Part can be text, function call, or function response
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum GeminiPart {
    Text(GeminiTextPart),
    FunctionCall(GeminiFunctionCallPart),
    FunctionResponse(GeminiFunctionResponsePart),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeminiTextPart {
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeminiFunctionCallPart {
    #[serde(rename = "functionCall")]
    pub function_call: GeminiFunctionCall,
    /// Thought signature - required for Gemini 3 multi-turn tool use
    #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeminiFunctionCall {
    pub name: String,
    pub args: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeminiFunctionResponsePart {
    #[serde(rename = "functionResponse")]
    pub function_response: GeminiFunctionResponse,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeminiFunctionResponse {
    pub name: String,
    pub response: Value,
}

#[derive(Serialize)]
struct GeminiTool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: Value,
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

#[derive(Deserialize, Clone, Debug)]
pub struct GeminiPartResponse {
    pub text: Option<String>,
    #[serde(rename = "functionCall")]
    pub function_call: Option<GeminiFunctionCallResponse>,
    #[serde(rename = "thoughtSignature")]
    pub thought_signature: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct GeminiFunctionCallResponse {
    pub name: String,
    pub args: Value,
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
// Tool Schema Generation
// ============================================================================

/// Convert Mira's allowed tools to Gemini function declarations
fn gemini_tool_declarations() -> Vec<GeminiFunctionDeclaration> {
    tool_bridge::AllowedTool::all()
        .into_iter()
        .map(|tool| {
            let schema = tool_bridge::openai_tool_schema(tool);
            // Extract function details from OpenAI schema
            let func = &schema["function"];
            GeminiFunctionDeclaration {
                name: func["name"].as_str().unwrap_or(tool.name()).to_string(),
                description: func["description"].as_str().unwrap_or(tool.description()).to_string(),
                parameters: func["parameters"].clone(),
            }
        })
        .collect()
}

// ============================================================================
// Provider Implementation
// ============================================================================

impl GeminiProvider {
    /// Complete with raw contents (for tool loop)
    ///
    /// Uses "low" thinking when tools are enabled for faster tool routing,
    /// "high" thinking for final response without tools.
    pub async fn complete_with_contents(
        &self,
        contents: Vec<GeminiContent>,
        system: Option<String>,
        enable_tools: bool,
    ) -> Result<(AdvisoryResponse, Vec<GeminiPartResponse>)> {
        let tools = if enable_tools {
            Some(vec![GeminiTool {
                function_declarations: gemini_tool_declarations(),
            }])
        } else {
            None
        };

        // Use "low" thinking for tool routing (faster), "high" for final response
        // Note: Gemini 3 Pro only supports "low" and "high" - no "medium"
        let thinking_level = if enable_tools { "low" } else { "high" };

        let api_request = GeminiRequest {
            contents,
            system_instruction: system.map(|s| GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: s }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: thinking_level.to_string(),
                },
            }),
            tools,
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

        // Extract text and function calls from parts
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCallRequest> = vec![];
        let mut raw_parts: Vec<GeminiPartResponse> = vec![];

        if let Some(candidates) = api_response.candidates {
            if let Some(candidate) = candidates.into_iter().next() {
                for part in candidate.content.parts {
                    raw_parts.push(part.clone());

                    if let Some(t) = &part.text {
                        text.push_str(t);
                    }

                    if let Some(fc) = &part.function_call {
                        tool_calls.push(ToolCallRequest {
                            id: format!("gemini_{}", tool_calls.len()),
                            name: fc.name.clone(),
                            arguments: fc.args.clone(),
                        });
                    }
                }
            }
        }

        let usage = api_response.usage_metadata.map(|u| AdvisoryUsage {
            input_tokens: u.prompt_token_count.unwrap_or(0),
            output_tokens: u.candidates_token_count.unwrap_or(0),
            reasoning_tokens: 0,
            cache_read_tokens: 0,  // Gemini 3 Pro preview: no caching yet
            cache_write_tokens: 0,
        });

        Ok((
            AdvisoryResponse {
                text,
                usage,
                model: AdvisoryModel::Gemini3Pro,
                tool_calls,
                reasoning: None,
            },
            raw_parts,
        ))
    }
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
                parts: vec![GeminiPart::Text(GeminiTextPart { text: msg.content.clone() })],
            });
        }

        // Add current message
        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::Text(GeminiTextPart { text: request.message })],
        });

        let tools = if request.enable_tools {
            Some(vec![GeminiTool {
                function_declarations: gemini_tool_declarations(),
            }])
        } else {
            None
        };

        let api_request = GeminiRequest {
            contents,
            system_instruction: request.system.map(|s| GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: s }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: "high".to_string(),
                },
            }),
            tools,
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

        // Extract text and function calls
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCallRequest> = vec![];

        if let Some(candidates) = api_response.candidates {
            if let Some(candidate) = candidates.into_iter().next() {
                for (idx, part) in candidate.content.parts.into_iter().enumerate() {
                    if let Some(t) = part.text {
                        text.push_str(&t);
                    }
                    if let Some(fc) = part.function_call {
                        tool_calls.push(ToolCallRequest {
                            id: format!("gemini_{}", idx),
                            name: fc.name,
                            arguments: fc.args,
                        });
                    }
                }
            }
        }

        let usage = api_response.usage_metadata.map(|u| AdvisoryUsage {
            input_tokens: u.prompt_token_count.unwrap_or(0),
            output_tokens: u.candidates_token_count.unwrap_or(0),
            reasoning_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        });

        Ok(AdvisoryResponse {
            text,
            usage,
            model: AdvisoryModel::Gemini3Pro,
            tool_calls,
            reasoning: None,
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
                parts: vec![GeminiPart::Text(GeminiTextPart { text: msg.content.clone() })],
            });
        }

        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::Text(GeminiTextPart { text: request.message })],
        });

        let api_request = GeminiRequest {
            contents,
            system_instruction: request.system.map(|s| GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: s }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: "high".to_string(),
                },
            }),
            tools: None, // Tools not supported in streaming mode yet
        };

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
// Gemini Input Item (for tool loop)
// ============================================================================

/// Input item for Gemini tool loop
#[derive(Clone, Debug)]
pub enum GeminiInputItem {
    /// User message
    UserMessage(String),
    /// Model response with function calls (includes thought signature)
    ModelFunctionCall {
        name: String,
        args: Value,
        thought_signature: Option<String>,
    },
    /// Function response
    FunctionResponse {
        name: String,
        response: Value,
    },
}

impl GeminiInputItem {
    /// Convert to GeminiContent for API request
    pub fn to_content(&self) -> GeminiContent {
        match self {
            GeminiInputItem::UserMessage(text) => GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart::Text(GeminiTextPart { text: text.clone() })],
            },
            GeminiInputItem::ModelFunctionCall { name, args, thought_signature } => GeminiContent {
                role: "model".to_string(),
                parts: vec![GeminiPart::FunctionCall(GeminiFunctionCallPart {
                    function_call: GeminiFunctionCall {
                        name: name.clone(),
                        args: args.clone(),
                    },
                    thought_signature: thought_signature.clone(),
                })],
            },
            GeminiInputItem::FunctionResponse { name, response } => GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart::FunctionResponse(GeminiFunctionResponsePart {
                    function_response: GeminiFunctionResponse {
                        name: name.clone(),
                        response: response.clone(),
                    },
                })],
            },
        }
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
