//! Opus 4.5 Provider (Anthropic) with tool calling support
//!
//! Uses Anthropic's Messages API with extended thinking.
//! Tool calling is compatible with extended thinking when tool_choice is "auto".

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
use crate::advisory::tool_loop::{ToolLoopProvider, ToolDefinition};

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
                supports_tools: true,     // Now implemented
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
    /// System prompt as content blocks (supports cache_control)
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<SystemBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

/// System prompt block with optional cache control
#[derive(Serialize, Clone, Debug)]
struct SystemBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

/// Cache control marker for prompt caching
#[derive(Serialize, Clone, Debug)]
struct CacheControl {
    #[serde(rename = "type")]
    cache_type: String,
}

impl SystemBlock {
    /// Create a cacheable system block
    fn cacheable(text: String) -> Self {
        Self {
            block_type: "text".to_string(),
            text,
            cache_control: Some(CacheControl {
                cache_type: "ephemeral".to_string(),
            }),
        }
    }

    /// Create a non-cached system block
    #[allow(dead_code)]
    fn plain(text: String) -> Self {
        Self {
            block_type: "text".to_string(),
            text,
            cache_control: None,
        }
    }
}

#[derive(Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    thinking_type: String,
    budget_tokens: u32,
}

#[derive(Serialize, Clone, Debug)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

/// Content can be a simple string or array of content blocks
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

/// Content block types for Anthropic API
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum AnthropicContentBlock {
    /// Text content
    Text {
        text: String,
    },
    /// Tool use request from Claude
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// Tool result from user
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    /// Thinking block (extended thinking)
    /// When sending back in multi-turn, the signature is required.
    Thinking {
        thinking: String,
        /// Signature from the original response (required for multi-turn)
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
}

/// Tool definition for Anthropic API
#[derive(Serialize, Clone, Debug)]
struct AnthropicTool {
    #[serde(rename = "type")]
    tool_type: String,
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Option<Vec<AnthropicResponseBlock>>,
    error: Option<AnthropicError>,
    usage: Option<AnthropicUsage>,
    stop_reason: Option<String>,
}

/// Response content block (different from request)
#[derive(Deserialize, Clone, Debug)]
pub struct AnthropicResponseBlock {
    #[serde(rename = "type")]
    content_type: String,
    // For text blocks
    text: Option<String>,
    // For tool_use blocks
    id: Option<String>,
    name: Option<String>,
    input: Option<Value>,
    // For thinking blocks
    thinking: Option<String>,
    /// Signature for thinking blocks (required for multi-turn with extended thinking)
    signature: Option<String>,
}

impl AnthropicResponseBlock {
    /// Check if this is a thinking block
    pub fn is_thinking(&self) -> bool {
        self.content_type == "thinking"
    }

    /// Get thinking content if this is a thinking block
    pub fn get_thinking(&self) -> Option<&str> {
        if self.is_thinking() {
            self.thinking.as_deref()
        } else {
            None
        }
    }

    /// Get thinking content with signature (required for multi-turn)
    pub fn get_thinking_with_signature(&self) -> Option<(&str, Option<&str>)> {
        if self.is_thinking() {
            self.thinking.as_deref().map(|t| (t, self.signature.as_deref()))
        } else {
            None
        }
    }
}

#[derive(Deserialize)]
struct AnthropicError {
    message: String,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
}

// ============================================================================
// Tool Schema Generation
// ============================================================================

/// Convert Mira's allowed tools to Anthropic tool format
fn anthropic_tool_definitions() -> Vec<AnthropicTool> {
    tool_bridge::AllowedTool::all()
        .into_iter()
        .map(|tool| {
            let schema = tool_bridge::openai_tool_schema(tool);
            AnthropicTool {
                tool_type: "custom".to_string(),
                name: schema["name"].as_str().unwrap_or(tool.name()).to_string(),
                description: schema["description"].as_str().unwrap_or(tool.description()).to_string(),
                input_schema: schema["parameters"].clone(),
            }
        })
        .collect()
}

// ============================================================================
// Input Item Types (for tool loop)
// ============================================================================

/// Input item for Opus tool loop
#[derive(Clone, Debug)]
pub enum OpusInputItem {
    /// User message
    UserMessage(String),
    /// Assistant message with optional tool use
    /// When extended thinking is enabled, assistant messages in multi-turn
    /// conversations must include the thinking block from the original response,
    /// including the signature which is required for API validation.
    AssistantMessage {
        thinking: Option<String>,
        /// Signature from the thinking block (required for multi-turn with extended thinking)
        thinking_signature: Option<String>,
        text: Option<String>,
        tool_uses: Vec<OpusToolUse>,
    },
    /// Tool result
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// Tool use from assistant response
#[derive(Clone, Debug)]
pub struct OpusToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

impl OpusInputItem {
    /// Convert to AnthropicMessage for API request
    pub fn to_message(&self) -> AnthropicMessage {
        match self {
            OpusInputItem::UserMessage(text) => AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text(text.clone()),
            },
            OpusInputItem::AssistantMessage { thinking, thinking_signature, text, tool_uses } => {
                let mut blocks = vec![];
                // Thinking block MUST come first when extended thinking is enabled
                // This is required by the Anthropic API for multi-turn conversations
                // The signature is required when sending thinking back for multi-turn
                if let Some(t) = thinking {
                    blocks.push(AnthropicContentBlock::Thinking {
                        thinking: t.clone(),
                        signature: thinking_signature.clone(),
                    });
                }
                if let Some(t) = text {
                    blocks.push(AnthropicContentBlock::Text { text: t.clone() });
                }
                for tu in tool_uses {
                    blocks.push(AnthropicContentBlock::ToolUse {
                        id: tu.id.clone(),
                        name: tu.name.clone(),
                        input: tu.input.clone(),
                    });
                }
                AnthropicMessage {
                    role: "assistant".to_string(),
                    content: AnthropicContent::Blocks(blocks),
                }
            }
            OpusInputItem::ToolResult { tool_use_id, content, is_error } => {
                AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Blocks(vec![
                        AnthropicContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: content.clone(),
                            is_error: if *is_error { Some(true) } else { None },
                        }
                    ]),
                }
            }
        }
    }
}

// ============================================================================
// Provider Implementation
// ============================================================================

impl OpusProvider {
    /// Complete with raw input items (for tool loop)
    ///
    /// Uses lower thinking budget when tools are enabled for faster tool routing,
    /// higher budget for final response without tools.
    pub async fn complete_with_items(
        &self,
        items: Vec<OpusInputItem>,
        system: Option<String>,
        enable_tools: bool,
    ) -> Result<(AdvisoryResponse, Vec<AnthropicResponseBlock>)> {
        let tools = if enable_tools {
            Some(anthropic_tool_definitions())
        } else {
            None
        };

        // Use lower thinking budget for tool routing (faster), higher for final response
        let thinking_budget = if enable_tools { 10000 } else { 32000 };

        let messages: Vec<AnthropicMessage> = items.iter().map(|i| i.to_message()).collect();

        // Convert system prompt to cacheable block
        let system_blocks = system.map(|s| vec![SystemBlock::cacheable(s)]);

        let api_request = AnthropicRequest {
            model: "claude-opus-4-5-20251101".to_string(),
            max_tokens: 64000,
            messages,
            system: system_blocks,
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".to_string(),
                budget_tokens: thinking_budget,
            }),
            tools,
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

        // Extract text and tool calls from content blocks
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCallRequest> = vec![];
        let mut raw_blocks: Vec<AnthropicResponseBlock> = vec![];

        if let Some(contents) = api_response.content {
            for block in contents {
                raw_blocks.push(block.clone());

                match block.content_type.as_str() {
                    "text" => {
                        if let Some(t) = block.text {
                            text.push_str(&t);
                        }
                    }
                    "tool_use" => {
                        if let (Some(id), Some(name), Some(input)) =
                            (block.id, block.name, block.input)
                        {
                            tool_calls.push(ToolCallRequest {
                                id,
                                name,
                                arguments: input,
                            });
                        }
                    }
                    "thinking" => {
                        // Skip thinking blocks - they're internal reasoning
                    }
                    _ => {}
                }
            }
        }

        let usage = api_response.usage.map(|u| AdvisoryUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            reasoning_tokens: 0, // Anthropic doesn't separate thinking tokens in usage
            cache_read_tokens: u.cache_read_input_tokens,
            cache_write_tokens: u.cache_creation_input_tokens,
        });

        Ok((
            AdvisoryResponse {
                text,
                usage,
                model: AdvisoryModel::Opus45,
                tool_calls,
                reasoning: None, // Opus thinking blocks are internal, not exposed here
            },
            raw_blocks,
        ))
    }
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
                content: AnthropicContent::Text(msg.content.clone()),
            });
        }

        // Add current message
        messages.push(AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicContent::Text(request.message),
        });

        // Build tools if enabled
        let tools = if request.enable_tools {
            Some(anthropic_tool_definitions())
        } else {
            None
        };

        // Use lower thinking budget when tools are enabled
        let thinking_budget = if request.enable_tools { 10000 } else { 32000 };

        // Convert system prompt to cacheable block
        let system_blocks = request.system.map(|s| vec![SystemBlock::cacheable(s)]);

        let api_request = AnthropicRequest {
            model: "claude-opus-4-5-20251101".to_string(),
            max_tokens: 64000,
            messages,
            system: system_blocks,
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".to_string(),
                budget_tokens: thinking_budget,
            }),
            tools,
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

        // Extract text and tool calls from content blocks
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCallRequest> = vec![];

        if let Some(contents) = api_response.content {
            for block in contents {
                match block.content_type.as_str() {
                    "text" => {
                        if let Some(t) = block.text {
                            text.push_str(&t);
                        }
                    }
                    "tool_use" => {
                        if let (Some(id), Some(name), Some(input)) =
                            (block.id, block.name, block.input)
                        {
                            tool_calls.push(ToolCallRequest {
                                id,
                                name,
                                arguments: input,
                            });
                        }
                    }
                    "thinking" => {
                        // Skip thinking blocks
                    }
                    _ => {}
                }
            }
        }

        let usage = api_response.usage.map(|u| AdvisoryUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            reasoning_tokens: 0,
            cache_read_tokens: u.cache_read_input_tokens,
            cache_write_tokens: u.cache_creation_input_tokens,
        });

        Ok(AdvisoryResponse {
            text,
            usage,
            model: AdvisoryModel::Opus45,
            tool_calls,
            reasoning: None,
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
                content: AnthropicContent::Text(msg.content.clone()),
            });
        }

        messages.push(AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicContent::Text(request.message),
        });

        // Convert system prompt to cacheable block
        let system_blocks = request.system.map(|s| vec![SystemBlock::cacheable(s)]);

        // Note: Streaming with tools is more complex - for now, tools only work with complete()
        let api_request = AnthropicRequest {
            model: "claude-opus-4-5-20251101".to_string(),
            max_tokens: 64000,
            messages,
            system: system_blocks,
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".to_string(),
                budget_tokens: 32000,
            }),
            tools: None, // Tools not supported in streaming mode yet
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

// ============================================================================
// ToolLoopProvider Implementation
// ============================================================================

/// Convert ToolDefinitions to Anthropic tool format
fn tool_definitions_to_anthropic(tools: &[ToolDefinition]) -> Vec<AnthropicTool> {
    tools
        .iter()
        .map(|td| {
            let schema = tool_bridge::openai_tool_schema(td.tool.clone());
            AnthropicTool {
                tool_type: "custom".to_string(),
                name: schema["name"].as_str().unwrap_or(td.tool.name()).to_string(),
                description: schema["description"].as_str().unwrap_or(td.tool.description()).to_string(),
                input_schema: schema["parameters"].clone(),
            }
        })
        .collect()
}

#[async_trait]
impl ToolLoopProvider for OpusProvider {
    /// State is a list of input items (user messages, assistant messages, tool results)
    type State = Vec<OpusInputItem>;

    /// Raw response preserves thinking blocks with signatures for multi-turn
    type RawResponse = Vec<AnthropicResponseBlock>;

    fn name(&self) -> &'static str {
        "Opus 4.5"
    }

    fn timeout_secs(&self) -> u64 {
        DEFAULT_TIMEOUT_SECS
    }

    fn init_conversation(&self, message: &str) -> Self::State {
        vec![OpusInputItem::UserMessage(message.to_string())]
    }

    async fn call(
        &self,
        state: &Self::State,
        system: Option<&str>,
        tools: Option<&[ToolDefinition]>,
    ) -> Result<(AdvisoryResponse, Self::RawResponse, AdvisoryUsage)> {
        let anthropic_tools = tools.map(tool_definitions_to_anthropic);
        let enable_tools = anthropic_tools.is_some();

        // Use lower thinking budget for tool routing (faster), higher for final response
        let thinking_budget = if enable_tools { 10000 } else { 32000 };

        let messages: Vec<AnthropicMessage> = state.iter().map(|i| i.to_message()).collect();

        // Convert system prompt to cacheable block
        let system_blocks = system.map(|s| vec![SystemBlock::cacheable(s.to_string())]);

        let api_request = AnthropicRequest {
            model: "claude-opus-4-5-20251101".to_string(),
            max_tokens: 64000,
            messages,
            system: system_blocks,
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".to_string(),
                budget_tokens: thinking_budget,
            }),
            tools: anthropic_tools,
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

        // Extract text and tool calls from content blocks
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCallRequest> = vec![];
        let mut raw_blocks: Vec<AnthropicResponseBlock> = vec![];

        if let Some(contents) = api_response.content {
            for block in contents {
                raw_blocks.push(block.clone());

                match block.content_type.as_str() {
                    "text" => {
                        if let Some(t) = block.text {
                            text.push_str(&t);
                        }
                    }
                    "tool_use" => {
                        if let (Some(id), Some(name), Some(input)) =
                            (block.id, block.name, block.input)
                        {
                            tool_calls.push(ToolCallRequest {
                                id,
                                name,
                                arguments: input,
                            });
                        }
                    }
                    "thinking" => {
                        // Skip thinking blocks in response text - they're internal reasoning
                    }
                    _ => {}
                }
            }
        }

        let usage = api_response.usage
            .map(|u| AdvisoryUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                reasoning_tokens: 0,
                cache_read_tokens: u.cache_read_input_tokens,
                cache_write_tokens: u.cache_creation_input_tokens,
            })
            .unwrap_or_default();

        Ok((
            AdvisoryResponse {
                text,
                usage: Some(usage.clone()),
                model: AdvisoryModel::Opus45,
                tool_calls,
                reasoning: None,
            },
            raw_blocks,
            usage,
        ))
    }

    fn add_assistant_response(
        &self,
        state: &mut Self::State,
        _response: &AdvisoryResponse,
        raw: &Self::RawResponse,
    ) {
        // Extract thinking block (if present) and tool uses from raw blocks
        let mut thinking: Option<String> = None;
        let mut thinking_signature: Option<String> = None;
        let mut text_content: Option<String> = None;
        let mut tool_uses = vec![];

        for block in raw {
            match block.content_type.as_str() {
                "thinking" => {
                    // Preserve thinking with signature for multi-turn
                    thinking = block.thinking.clone();
                    thinking_signature = block.signature.clone();
                }
                "text" => {
                    if let Some(t) = &block.text {
                        text_content = Some(t.clone());
                    }
                }
                "tool_use" => {
                    if let (Some(id), Some(name), Some(input)) =
                        (&block.id, &block.name, &block.input)
                    {
                        tool_uses.push(OpusToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        // Only add if there's something to add
        if thinking.is_some() || text_content.is_some() || !tool_uses.is_empty() {
            state.push(OpusInputItem::AssistantMessage {
                thinking,
                thinking_signature,
                text: text_content,
                tool_uses,
            });
        }
    }

    fn add_tool_results(
        &self,
        state: &mut Self::State,
        results: Vec<tool_bridge::ToolResult>,
    ) {
        for result in results {
            state.push(OpusInputItem::ToolResult {
                tool_use_id: result.tool_call_id,
                content: result.content,
                is_error: result.is_error,
            });
        }
    }
}
