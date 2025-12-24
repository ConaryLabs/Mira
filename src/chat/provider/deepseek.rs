//! DeepSeek provider implementation (Chat Completions API)
//!
//! Implements the OpenAI-compatible Chat Completions API for DeepSeek models.
//! Uses core::SseDecoder for SSE stream parsing.

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use crate::core::SseDecoder;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::{
    Capabilities, ChatRequest, ChatResponse, FinishReason, MessageRole, Provider,
    StreamEvent, ToolCall, ToolContinueRequest, ToolDefinition, Usage,
};

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/v1/chat/completions";

/// DeepSeek provider using Chat Completions API
pub struct DeepSeekProvider {
    client: HttpClient,
    api_key: String,
    capabilities: Capabilities,
    #[allow(dead_code)] // For future model selection (deepseek-chat, deepseek-reasoner, etc.)
    model: String,
}

impl DeepSeekProvider {
    /// Create a new DeepSeek Chat provider
    pub fn new_chat(api_key: String) -> Self {
        Self {
            client: HttpClient::new(),
            api_key,
            capabilities: Capabilities::deepseek_chat(),
            model: "deepseek-chat".into(),
        }
    }

    /// Create a new DeepSeek Reasoner provider
    pub fn new_reasoner(api_key: String) -> Self {
        Self {
            client: HttpClient::new(),
            api_key,
            capabilities: Capabilities::deepseek_reasoner(),
            model: "deepseek-reasoner".into(),
        }
    }

    /// Build message list from request
    fn build_messages(request: &ChatRequest) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        // System message
        messages.push(ChatMessage {
            role: "system".into(),
            content: Some(request.system.clone()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        });

        // History messages (for client-state)
        for msg in &request.messages {
            messages.push(ChatMessage {
                role: msg.role.as_str().into(),
                content: Some(msg.content.clone()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Current user input
        messages.push(ChatMessage {
            role: "user".into(),
            content: Some(request.input.clone()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        });

        messages
    }

    /// Build messages for tool continuation
    fn build_tool_messages(request: &ToolContinueRequest) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        // System message
        messages.push(ChatMessage {
            role: "system".into(),
            content: Some(request.system.clone()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        });

        // History messages
        for msg in &request.messages {
            messages.push(ChatMessage {
                role: msg.role.as_str().into(),
                content: Some(msg.content.clone()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Assistant message with tool_calls (required before tool results)
        // Reconstruct from tool_results - the API requires this ordering
        // CRITICAL: Include reasoning_content from the response that produced these tool calls
        // DeepSeek Reasoner requires this for continued reasoning (omission triggers 400 errors)
        if !request.tool_results.is_empty() {
            let tool_calls: Vec<ChatToolCall> = request
                .tool_results
                .iter()
                .map(|r| ChatToolCall {
                    id: r.call_id.clone(),
                    call_type: "function".into(),
                    function: ChatToolCallFunction {
                        name: r.name.clone(),
                        arguments: "{}".into(), // Args already executed, just need structure
                    },
                })
                .collect();

            messages.push(ChatMessage {
                role: "assistant".into(),
                content: None,
                reasoning_content: request.reasoning_content.clone(),
                tool_calls: Some(tool_calls),
                tool_call_id: None,
            });
        }

        // Tool results as tool messages
        for result in &request.tool_results {
            messages.push(ChatMessage {
                role: "tool".into(),
                content: Some(result.output.clone()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: Some(result.call_id.clone()),
            });
        }

        // Add user nudge to force response after tool results
        // Without this, DeepSeek often ends the turn silently after multi-tool chains
        // Reference the original question to help the model focus on answering it
        let original_question = request.messages.last()
            .filter(|m| m.role == MessageRole::User)
            .map(|m| m.content.chars().take(200).collect::<String>())
            .unwrap_or_default();

        let nudge = if original_question.is_empty() {
            "Now use the tool results above to write your response.".to_string()
        } else {
            format!(
                "The tool results are above. Now answer the original question: \"{}\"\n\
                 Write your response now (no more tool calls unless absolutely necessary).",
                original_question
            )
        };

        messages.push(ChatMessage {
            role: "user".into(),
            content: Some(nudge),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        });

        messages
    }

    /// Convert our tool definitions to OpenAI format
    fn convert_tools(tools: &[ToolDefinition]) -> Vec<ChatTool> {
        tools
            .iter()
            .map(|t| ChatTool {
                tool_type: "function".into(),
                function: ChatFunction {
                    name: t.name.clone(),
                    description: Some(t.description.clone()),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    }

    /// Process SSE stream and send events to channel
    ///
    /// Shared logic for both create_stream and continue_with_tools_stream.
    /// Uses SseDecoder from mira-core for consistent SSE parsing.
    ///
    /// Tracks multiple parallel tool calls by index to handle DeepSeek's
    /// interleaved streaming of multiple tool calls in one response.
    async fn process_sse_stream(
        response: reqwest::Response,
        tx: mpsc::Sender<StreamEvent>,
    ) {
        // Track in-flight tool calls by index: (id, name, args, started)
        struct InFlightCall {
            id: String,
            name: String,
            args: String,
            started: bool,
        }

        let mut stream = response.bytes_stream();
        let mut decoder = SseDecoder::new();
        let mut tool_calls: HashMap<usize, InFlightCall> = HashMap::new();

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                    break;
                }
            };

            // Use SseDecoder to parse SSE frames
            for frame in decoder.push(&chunk) {
                if frame.is_done() {
                    continue;
                }

                // Parse as ChatStreamChunk
                let chunk_data: ChatStreamChunk = match frame.try_parse() {
                    Some(c) => c,
                    None => continue,
                };

                for choice in chunk_data.choices {
                    let delta = choice.delta;

                    // Handle text content
                    if let Some(content) = delta.content {
                        if !content.is_empty() {
                            let _ = tx.send(StreamEvent::TextDelta(content)).await;
                        }
                    }

                    // Handle reasoning content (DeepSeek reasoner)
                    if let Some(reasoning) = delta.reasoning_content {
                        if !reasoning.is_empty() {
                            let _ = tx.send(StreamEvent::ReasoningDelta(reasoning)).await;
                        }
                    }

                    // Handle tool calls - track by index for parallel calls
                    if let Some(delta_tool_calls) = delta.tool_calls {
                        for tc in delta_tool_calls {
                            let idx = tc.index;

                            // Get or create the in-flight call for this index
                            let call = tool_calls.entry(idx).or_insert_with(|| InFlightCall {
                                id: String::new(),
                                name: String::new(),
                                args: String::new(),
                                started: false,
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
                            }

                            // Emit FunctionCallStart once we have both id and name
                            if !call.started && !call.id.is_empty() && !call.name.is_empty() {
                                call.started = true;
                                let _ = tx
                                    .send(StreamEvent::FunctionCallStart {
                                        call_id: call.id.clone(),
                                        name: call.name.clone(),
                                    })
                                    .await;
                            }

                            // Accumulate arguments delta
                            if let Some(ref func) = tc.function {
                                if let Some(ref args) = func.arguments {
                                    if !args.is_empty() {
                                        call.args.push_str(args);
                                        if call.started {
                                            let _ = tx
                                                .send(StreamEvent::FunctionCallDelta {
                                                    call_id: call.id.clone(),
                                                    arguments_delta: args.clone(),
                                                })
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Handle finish - emit FunctionCallEnd for all pending calls
                    if choice.finish_reason.is_some() {
                        for (_, call) in tool_calls.drain() {
                            if call.started {
                                let _ = tx
                                    .send(StreamEvent::FunctionCallEnd { call_id: call.id })
                                    .await;
                            }
                        }
                    }
                }

                // Usage info
                if let Some(usage) = chunk_data.usage {
                    let _ = tx
                        .send(StreamEvent::Usage(Usage {
                            input_tokens: usage.prompt_tokens,
                            output_tokens: usage.completion_tokens,
                            reasoning_tokens: usage.reasoning_tokens.unwrap_or(0),
                            cached_tokens: usage.prompt_cache_hit_tokens.unwrap_or(0),
                        }))
                        .await;
                }
            }
        }

        let _ = tx.send(StreamEvent::Done).await;
    }
}

#[async_trait]
impl Provider for DeepSeekProvider {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn name(&self) -> &'static str {
        "deepseek"
    }

    async fn create_stream(
        &self,
        request: ChatRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let messages = Self::build_messages(&request);
        let tools = if self.capabilities.supports_tools && !request.tools.is_empty() {
            Some(Self::convert_tools(&request.tools))
        } else {
            None
        };

        let body = ChatCompletionRequest {
            model: request.model.clone(),
            messages,
            tools,
            stream: true,
            max_tokens: request.max_tokens,
        };

        let response = self
            .client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|e| format!("(failed to read body: {})", e));
            anyhow::bail!("DeepSeek API error {}: {}", status, text);
        }

        let (tx, rx) = mpsc::channel(100);

        // Spawn task to process SSE stream using shared helper
        tokio::spawn(Self::process_sse_stream(response, tx));

        Ok(rx)
    }

    async fn create(&self, request: ChatRequest) -> Result<ChatResponse> {
        let messages = Self::build_messages(&request);
        let tools = if self.capabilities.supports_tools && !request.tools.is_empty() {
            Some(Self::convert_tools(&request.tools))
        } else {
            None
        };

        let body = ChatCompletionRequest {
            model: request.model.clone(),
            messages,
            tools,
            stream: false,
            max_tokens: request.max_tokens,
        };

        let response = self
            .client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|e| format!("(failed to read body: {})", e));
            anyhow::bail!("DeepSeek API error {}: {}", status, text);
        }

        let result: ChatCompletionResponse = response.json().await?;

        let choice = result.choices.first().ok_or_else(|| anyhow::anyhow!("No choices in response"))?;

        let text = choice.message.content.clone().unwrap_or_default();
        let reasoning = choice.message.reasoning_content.clone();

        let tool_calls = choice
            .message
            .tool_calls
            .as_ref()
            .map(|tcs| {
                tcs.iter()
                    .map(|tc| ToolCall {
                        call_id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let finish_reason = match choice.finish_reason.as_deref() {
            Some("tool_calls") => FinishReason::ToolCalls,
            Some("length") => FinishReason::Length,
            Some("content_filter") => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        };

        let usage = result.usage.map(|u| Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            reasoning_tokens: u.reasoning_tokens.unwrap_or(0),
            cached_tokens: u.prompt_cache_hit_tokens.unwrap_or(0),
        });

        Ok(ChatResponse {
            id: result.id,
            text,
            reasoning,
            tool_calls,
            usage,
            finish_reason,
        })
    }

    async fn continue_with_tools_stream(
        &self,
        request: ToolContinueRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let messages = Self::build_tool_messages(&request);
        let tools = if self.capabilities.supports_tools && !request.tools.is_empty() {
            Some(Self::convert_tools(&request.tools))
        } else {
            None
        };

        let body = ChatCompletionRequest {
            model: request.model.clone(),
            messages,
            tools,
            stream: true,
            max_tokens: None,
        };

        let response = self
            .client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|e| format!("(failed to read body: {})", e));
            anyhow::bail!("DeepSeek API error {}: {}", status, text);
        }

        // Reuse the shared streaming logic
        let (tx, rx) = mpsc::channel(100);
        tokio::spawn(Self::process_sse_stream(response, tx));

        Ok(rx)
    }
}

// ============================================================================
// DeepSeek API Types (OpenAI-compatible Chat Completions format)
// ============================================================================

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    /// Reasoning content for DeepSeek Reasoner - must be passed back during tool continuations
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
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

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    id: String,
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ChatToolCall>>,
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

#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    reasoning_tokens: Option<u32>,
    /// DeepSeek reports cached tokens via prompt_cache_hit_tokens
    prompt_cache_hit_tokens: Option<u32>,
}

// Streaming types
#[derive(Debug, Deserialize)]
struct ChatStreamChunk {
    choices: Vec<ChatStreamChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamChoice {
    delta: ChatStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ChatStreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamToolCall {
    #[serde(default)]
    index: usize,
    id: Option<String>,
    function: Option<ChatStreamFunction>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}
