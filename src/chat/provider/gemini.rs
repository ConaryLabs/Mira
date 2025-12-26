//! Gemini 3 Pro provider for Studio chat (Orchestrator mode)
//!
//! Uses Gemini's generateContent API with function calling.
//! Adapted from advisory/providers/gemini.rs for the chat Provider interface.

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{
    Capabilities, ChatRequest, ChatResponse, FinishReason, Provider,
    StreamEvent, ToolCall, ToolContinueRequest, ToolDefinition, Usage,
};

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-preview:generateContent";
const GEMINI_STREAM_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-preview:streamGenerateContent";
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Gemini 3 Pro provider for chat interface
pub struct GeminiChatProvider {
    client: HttpClient,
    api_key: String,
    capabilities: Capabilities,
}

impl GeminiChatProvider {
    /// Create a new Gemini Chat provider
    pub fn new(api_key: String) -> Self {
        Self {
            client: HttpClient::new(),
            api_key,
            capabilities: Capabilities::gemini_3_pro(),
        }
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| anyhow::anyhow!("GEMINI_API_KEY not set"))?;
        Ok(Self::new(api_key))
    }

    /// Build Gemini contents from chat request
    fn build_contents(request: &ChatRequest) -> Vec<GeminiContent> {
        let mut contents = Vec::new();

        // Add history messages
        for msg in &request.messages {
            let role = match msg.role.as_str() {
                "user" => "user",
                "assistant" => "model",
                _ => continue, // Skip system/tool messages in history
            };
            contents.push(GeminiContent {
                role: role.to_string(),
                parts: vec![GeminiPart::Text { text: msg.content.clone() }],
            });
        }

        // Add current user input
        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::Text { text: request.input.clone() }],
        });

        contents
    }

    /// Build Gemini contents for tool continuation
    fn build_tool_contents(request: &ToolContinueRequest) -> Vec<GeminiContent> {
        let mut contents = Vec::new();

        // Add history messages
        for msg in &request.messages {
            let role = match msg.role.as_str() {
                "user" => "user",
                "assistant" => "model",
                _ => continue,
            };
            contents.push(GeminiContent {
                role: role.to_string(),
                parts: vec![GeminiPart::Text { text: msg.content.clone() }],
            });
        }

        // Add assistant message with tool calls (reconstructed)
        if !request.tool_results.is_empty() {
            let mut parts = Vec::new();
            for result in &request.tool_results {
                parts.push(GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: result.name.clone(),
                        args: serde_json::from_str(&result.output)
                            .unwrap_or(Value::Object(Default::default())),
                    },
                });
            }
            contents.push(GeminiContent {
                role: "model".to_string(),
                parts,
            });

            // Add tool results as user message with function responses
            let mut response_parts = Vec::new();
            for result in &request.tool_results {
                response_parts.push(GeminiPart::FunctionResponse {
                    function_response: GeminiFunctionResponse {
                        name: result.name.clone(),
                        response: serde_json::json!({ "result": result.output }),
                    },
                });
            }
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: response_parts,
            });
        }

        contents
    }

    /// Convert tool definitions to Gemini format
    fn build_tools(tools: &[ToolDefinition]) -> Option<Vec<GeminiTool>> {
        if tools.is_empty() {
            return None;
        }

        let declarations: Vec<GeminiFunctionDeclaration> = tools
            .iter()
            .map(|t| GeminiFunctionDeclaration {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            })
            .collect();

        Some(vec![GeminiTool { function_declarations: declarations }])
    }

    /// Make a non-streaming request
    async fn make_request(
        &self,
        contents: Vec<GeminiContent>,
        system: Option<String>,
        tools: Option<Vec<GeminiTool>>,
        thinking_level: &str,
    ) -> Result<GeminiResponse> {
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

        if let Some(error) = &api_response.error {
            anyhow::bail!("Gemini error: {}", error.message);
        }

        Ok(api_response)
    }

    /// Parse response into ChatResponse
    fn parse_response(response: GeminiResponse) -> ChatResponse {
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        let mut finish_reason = FinishReason::Stop;

        if let Some(candidates) = response.candidates {
            if let Some(candidate) = candidates.into_iter().next() {
                for part in candidate.content.parts {
                    if let Some(t) = part.text {
                        text.push_str(&t);
                    }
                    if let Some(fc) = part.function_call {
                        finish_reason = FinishReason::ToolCalls;
                        tool_calls.push(ToolCall {
                            call_id: format!("gemini_{}", tool_calls.len()),
                            name: fc.name,
                            arguments: fc.args.to_string(),
                        });
                    }
                }
            }
        }

        let usage = response.usage_metadata.map(|u| Usage {
            input_tokens: u.prompt_token_count.unwrap_or(0),
            output_tokens: u.candidates_token_count.unwrap_or(0),
            reasoning_tokens: 0,
            cached_tokens: 0,
        });

        ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            text,
            reasoning: None,
            tool_calls,
            usage,
            finish_reason,
        }
    }
}

#[async_trait]
impl Provider for GeminiChatProvider {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn name(&self) -> &'static str {
        "Gemini 3 Pro"
    }

    async fn create(&self, request: ChatRequest) -> Result<ChatResponse> {
        let contents = Self::build_contents(&request);
        let tools = Self::build_tools(&request.tools);
        let thinking_level = if tools.is_some() { "low" } else { "high" };

        let response = self.make_request(
            contents,
            Some(request.system),
            tools,
            thinking_level,
        ).await?;

        Ok(Self::parse_response(response))
    }

    async fn create_stream(
        &self,
        request: ChatRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let contents = Self::build_contents(&request);
        let tools = Self::build_tools(&request.tools);
        let thinking_level = if tools.is_some() { "low" } else { "high" };

        let api_request = GeminiRequest {
            contents,
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: request.system }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: thinking_level.to_string(),
                },
            }),
            tools,
        };

        let url = format!("{}?alt=sse&key={}", GEMINI_STREAM_URL, self.api_key);
        let client = self.client.clone();

        tokio::spawn(async move {
            match client
                .post(&url)
                .json(&api_request)
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx.send(StreamEvent::Error(
                            format!("Gemini API error: {} - {}", status, body)
                        )).await;
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    let mut tool_call_count = 0;

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                // Parse SSE events
                                while let Some(line_end) = buffer.find('\n') {
                                    let line = buffer[..line_end].to_string();
                                    buffer = buffer[line_end + 1..].to_string();

                                    if line.starts_with("data: ") {
                                        let data = &line[6..];
                                        if let Ok(response) = serde_json::from_str::<GeminiResponse>(data) {
                                            if let Some(candidates) = response.candidates {
                                                for candidate in candidates {
                                                    for part in candidate.content.parts {
                                                        if let Some(text) = part.text {
                                                            let _ = tx.send(StreamEvent::TextDelta(text)).await;
                                                        }
                                                        if let Some(fc) = part.function_call {
                                                            let call_id = format!("gemini_{}", tool_call_count);
                                                            tool_call_count += 1;
                                                            let _ = tx.send(StreamEvent::FunctionCallStart {
                                                                call_id: call_id.clone(),
                                                                name: fc.name.clone(),
                                                            }).await;
                                                            let _ = tx.send(StreamEvent::FunctionCallDelta {
                                                                call_id: call_id.clone(),
                                                                arguments_delta: fc.args.to_string(),
                                                            }).await;
                                                            let _ = tx.send(StreamEvent::FunctionCallEnd {
                                                                call_id,
                                                            }).await;
                                                        }
                                                    }
                                                }
                                            }
                                            if let Some(usage) = response.usage_metadata {
                                                let _ = tx.send(StreamEvent::Usage(Usage {
                                                    input_tokens: usage.prompt_token_count.unwrap_or(0),
                                                    output_tokens: usage.candidates_token_count.unwrap_or(0),
                                                    reasoning_tokens: 0,
                                                    cached_tokens: 0,
                                                })).await;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }

                    let _ = tx.send(StreamEvent::Done).await;
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }

    async fn continue_with_tools_stream(
        &self,
        request: ToolContinueRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let contents = Self::build_tool_contents(&request);
        let tools = Self::build_tools(&request.tools);
        let thinking_level = if tools.is_some() { "low" } else { "high" };

        let api_request = GeminiRequest {
            contents,
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: request.system }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: thinking_level.to_string(),
                },
            }),
            tools,
        };

        let url = format!("{}?alt=sse&key={}", GEMINI_STREAM_URL, self.api_key);
        let client = self.client.clone();

        tokio::spawn(async move {
            match client
                .post(&url)
                .json(&api_request)
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx.send(StreamEvent::Error(
                            format!("Gemini API error: {} - {}", status, body)
                        )).await;
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    let mut tool_call_count = 0;

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                while let Some(line_end) = buffer.find('\n') {
                                    let line = buffer[..line_end].to_string();
                                    buffer = buffer[line_end + 1..].to_string();

                                    if line.starts_with("data: ") {
                                        let data = &line[6..];
                                        if let Ok(response) = serde_json::from_str::<GeminiResponse>(data) {
                                            if let Some(candidates) = response.candidates {
                                                for candidate in candidates {
                                                    for part in candidate.content.parts {
                                                        if let Some(text) = part.text {
                                                            let _ = tx.send(StreamEvent::TextDelta(text)).await;
                                                        }
                                                        if let Some(fc) = part.function_call {
                                                            let call_id = format!("gemini_{}", tool_call_count);
                                                            tool_call_count += 1;
                                                            let _ = tx.send(StreamEvent::FunctionCallStart {
                                                                call_id: call_id.clone(),
                                                                name: fc.name.clone(),
                                                            }).await;
                                                            let _ = tx.send(StreamEvent::FunctionCallDelta {
                                                                call_id: call_id.clone(),
                                                                arguments_delta: fc.args.to_string(),
                                                            }).await;
                                                            let _ = tx.send(StreamEvent::FunctionCallEnd {
                                                                call_id,
                                                            }).await;
                                                        }
                                                    }
                                                }
                                            }
                                            if let Some(usage) = response.usage_metadata {
                                                let _ = tx.send(StreamEvent::Usage(Usage {
                                                    input_tokens: usage.prompt_token_count.unwrap_or(0),
                                                    output_tokens: usage.candidates_token_count.unwrap_or(0),
                                                    reasoning_tokens: 0,
                                                    cached_tokens: 0,
                                                })).await;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }

                    let _ = tx.send(StreamEvent::Done).await;
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
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
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Clone)]
#[serde(untagged)]
enum GeminiPart {
    Text { text: String },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Serialize, Deserialize, Clone)]
struct GeminiFunctionCall {
    name: String,
    args: Value,
}

#[derive(Serialize, Clone)]
struct GeminiFunctionResponse {
    name: String,
    response: Value,
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

#[derive(Serialize)]
struct GeminiTextPart {
    text: String,
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
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCallResponse>,
}

#[derive(Deserialize)]
struct GeminiFunctionCallResponse {
    name: String,
    args: Value,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities() {
        let provider = GeminiChatProvider::new("test_key".into());
        assert!(provider.capabilities().supports_tools);
        assert!(provider.capabilities().supports_streaming);
        assert_eq!(provider.capabilities().max_context_tokens, 1_000_000);
    }

    #[test]
    fn test_build_contents() {
        use super::super::Message;
        use super::super::MessageRole;

        let request = ChatRequest {
            model: "gemini-3-pro".into(),
            system: "You are helpful".into(),
            messages: vec![
                Message { role: MessageRole::User, content: "Hello".into() },
                Message { role: MessageRole::Assistant, content: "Hi there!".into() },
            ],
            input: "How are you?".into(),
            previous_response_id: None,
            reasoning_effort: None,
            tools: vec![],
            max_tokens: None,
        };

        let contents = GeminiChatProvider::build_contents(&request);
        assert_eq!(contents.len(), 3); // 2 history + 1 current
        assert_eq!(contents[0].role, "user");
        assert_eq!(contents[1].role, "model");
        assert_eq!(contents[2].role, "user");
    }
}
