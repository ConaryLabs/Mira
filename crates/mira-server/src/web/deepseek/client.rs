// crates/mira-server/src/web/deepseek/client.rs
// DeepSeek API client with streaming support

use super::types::{ChatResult, FunctionCall, Message, Tool, ToolCall, Usage};
use anyhow::{anyhow, Result};
use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{debug, error, info, instrument, warn, Span};
use uuid::Uuid;

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/chat/completions";

/// Check if content looks like garbage (JSON fragments, brackets, etc.)
/// Returns true if content should be streamed to UI, false if it should be suppressed
pub(super) fn is_streamable_content(content: &str) -> bool {
    let trimmed = content.trim();

    // Empty or whitespace-only
    if trimmed.is_empty() {
        return false;
    }

    // Single brackets/braces - likely tool call leakage
    if matches!(trimmed, "[" | "]" | "{" | "}" | "[]" | "{}") {
        return false;
    }

    // Just JSON punctuation
    if trimmed
        .chars()
        .all(|c| matches!(c, '[' | ']' | '{' | '}' | ',' | ':' | '"' | ' ' | '\n' | '\t'))
    {
        return false;
    }

    // Short content starting with JSON structure - likely tool call JSON fragment
    if (trimmed.starts_with('[') || trimmed.starts_with('{')) && trimmed.len() < 100 {
        // But allow markdown lists that start with [
        if !trimmed.contains("](") {
            return false;
        }
    }

    true
}

/// Chat completion request
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>, // "auto" | "required" | "none"
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// Streaming chunk
#[derive(Debug, Deserialize)]
struct ChatChunk {
    choices: Vec<ChunkChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
}

#[derive(Debug, Deserialize, Default)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallChunk>>,
}

/// Partial tool call in streaming
#[derive(Debug, Deserialize)]
struct ToolCallChunk {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionChunk>,
}

#[derive(Debug, Deserialize, Default)]
struct FunctionChunk {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// Non-streaming response for simple chat
#[derive(Debug, Deserialize)]
struct SimpleChatResponse {
    choices: Vec<SimpleChoice>,
    #[allow(dead_code)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct SimpleChoice {
    message: SimpleMessage,
}

#[derive(Debug, Deserialize)]
struct SimpleMessage {
    content: Option<String>,
}

/// DeepSeek API client
pub struct DeepSeekClient {
    api_key: String,
    client: reqwest::Client,
}

impl DeepSeekClient {
    /// Create a new DeepSeek client
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Simple non-streaming chat using deepseek-chat model (fast, cheap)
    /// Used for summarization, query generation, etc.
    pub async fn chat_simple(&self, system: &str, user: &str) -> Result<String> {
        let start_time = Instant::now();

        let request = serde_json::json!({
            "model": "deepseek-chat",
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "stream": false,
            "max_tokens": 2048
        });

        debug!("DeepSeek chat_simple request");

        let response = self
            .client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow!("Request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("DeepSeek API error {}: {}", status, body));
        }

        let data: SimpleChatResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        let content = data
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        let duration_ms = start_time.elapsed().as_millis();
        info!(
            duration_ms = duration_ms,
            content_len = content.len(),
            "DeepSeek chat_simple complete"
        );

        Ok(content)
    }

    /// Chat with streaming, returns the complete response
    #[instrument(skip(self, messages, tools), fields(request_id, model = "deepseek-reasoner", message_count = messages.len()))]
    pub async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        // Record request ID in span
        Span::current().record("request_id", &request_id);

        info!(
            request_id = %request_id,
            message_count = messages.len(),
            tool_count = tools.as_ref().map(|t| t.len()).unwrap_or(0),
            "Starting DeepSeek chat request"
        );

        let request = ChatRequest {
            model: "deepseek-reasoner".into(),
            messages,
            tools,
            tool_choice: Some("auto".into()),
            stream: true,
            max_tokens: Some(8192),
        };

        debug!(request_id = %request_id, "DeepSeek request: {:?}", serde_json::to_string(&request)?);

        let request_builder = self
            .client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request);

        // Stream the response using EventSource
        let mut es = match EventSource::new(request_builder) {
            Ok(es) => es,
            Err(e) => {
                error!(request_id = %request_id, error = %e, "Failed to create EventSource");
                return Err(anyhow!("Failed to create EventSource: {}", e));
            }
        };

        debug!(request_id = %request_id, "DeepSeek stream opened");

        let mut full_content = String::new();
        let mut full_reasoning = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage: Option<Usage> = None;
        let mut chunk_count = 0u32;

        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Open) => {
                    debug!(request_id = %request_id, "DeepSeek SSE connection opened");
                }
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        debug!(
                            request_id = %request_id,
                            chunks = chunk_count,
                            duration_ms = start_time.elapsed().as_millis(),
                            "DeepSeek stream complete"
                        );
                        break;
                    }

                    chunk_count += 1;
                    let chunk: ChatChunk = match serde_json::from_str(&msg.data) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!(
                                request_id = %request_id,
                                error = %e,
                                chunk = chunk_count,
                                "Failed to parse chunk"
                            );
                            debug!(request_id = %request_id, raw_data = %msg.data, "Raw chunk data");
                            continue;
                        }
                    };

                    // Capture usage from final chunk
                    if let Some(u) = chunk.usage {
                        info!(
                            request_id = %request_id,
                            prompt_tokens = u.prompt_tokens,
                            completion_tokens = u.completion_tokens,
                            cache_hit = ?u.prompt_cache_hit_tokens,
                            cache_miss = ?u.prompt_cache_miss_tokens,
                            "DeepSeek usage stats"
                        );
                        usage = Some(u);
                    }

                    for choice in chunk.choices {
                        // Accumulate reasoning content (don't stream - too many events)
                        if let Some(reasoning) = choice.delta.reasoning_content {
                            if !reasoning.is_empty() {
                                full_reasoning.push_str(&reasoning);
                            }
                        }

                        // Accumulate content
                        if let Some(content) = choice.delta.content {
                            if !content.is_empty() {
                                full_content.push_str(&content);
                            }
                        }

                        // Accumulate tool calls
                        if let Some(tc_chunks) = choice.delta.tool_calls {
                            for tc_chunk in tc_chunks {
                                // Ensure we have space for this tool call
                                while tool_calls.len() <= tc_chunk.index {
                                    tool_calls.push(ToolCall {
                                        id: String::new(),
                                        call_type: "function".into(),
                                        function: FunctionCall {
                                            name: String::new(),
                                            arguments: String::new(),
                                        },
                                    });
                                }

                                let tc = &mut tool_calls[tc_chunk.index];

                                if let Some(id) = tc_chunk.id {
                                    tc.id = id;
                                }
                                if let Some(func) = tc_chunk.function {
                                    if let Some(name) = func.name {
                                        tc.function.name = name;
                                    }
                                    if let Some(args) = func.arguments {
                                        tc.function.arguments.push_str(&args);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(
                        request_id = %request_id,
                        error = ?e,
                        chunks_received = chunk_count,
                        "DeepSeek stream error"
                    );
                    break;
                }
            }
        }

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Log tool calls if any
        if !tool_calls.is_empty() {
            info!(
                request_id = %request_id,
                tool_count = tool_calls.len(),
                tools = ?tool_calls.iter().map(|tc| &tc.function.name).collect::<Vec<_>>(),
                "DeepSeek requested tool calls"
            );
        }

        // Log tool calls (don't broadcast - causes UI flooding)
        for tc in &tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
            debug!(
                request_id = %request_id,
                tool = %tc.function.name,
                call_id = %tc.id,
                args = %args,
                "Tool call"
            );
        }

        info!(
            request_id = %request_id,
            duration_ms = duration_ms,
            content_len = full_content.len(),
            reasoning_len = full_reasoning.len(),
            tool_calls = tool_calls.len(),
            "DeepSeek chat complete"
        );

        Ok(ChatResult {
            request_id,
            content: if full_content.is_empty() {
                None
            } else {
                Some(full_content)
            },
            reasoning_content: if full_reasoning.is_empty() {
                None
            } else {
                Some(full_reasoning)
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage,
            duration_ms,
        })
    }

    /// Chat with streaming to a channel (for SSE endpoints)
    /// Sends Delta events to the channel instead of WebSocket broadcast
    pub async fn chat_to_channel(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
        tx: tokio::sync::mpsc::Sender<crate::web::chat::stream::ChatEvent>,
    ) -> Result<ChatResult> {
        use crate::web::chat::stream::ChatEvent;

        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        let request = ChatRequest {
            model: "deepseek-reasoner".into(),
            messages,
            tools,
            tool_choice: Some("auto".into()),
            stream: true,
            max_tokens: Some(8192),
        };

        let request_builder = self
            .client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request);

        let mut es = EventSource::new(request_builder)
            .map_err(|e| anyhow!("Failed to create EventSource: {}", e))?;

        let mut full_content = String::new();
        let mut full_reasoning = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage: Option<Usage> = None;
        let mut sent_thinking = false;
        let mut last_tool_names: Vec<String> = Vec::new();

        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Open) => {}
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        break;
                    }

                    let chunk: ChatChunk = match serde_json::from_str(&msg.data) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    if let Some(u) = chunk.usage {
                        usage = Some(u);
                    }

                    for choice in chunk.choices {
                        // Send Thinking event on first reasoning content
                        if let Some(reasoning) = choice.delta.reasoning_content {
                            if !reasoning.is_empty() {
                                if !sent_thinking {
                                    sent_thinking = true;
                                    let _ = tx.send(ChatEvent::Thinking).await;
                                }
                                full_reasoning.push_str(&reasoning);
                            }
                        }

                        // Stream content deltas to channel
                        if let Some(content) = choice.delta.content {
                            if !content.is_empty() {
                                full_content.push_str(&content);
                                // Filter garbage before sending
                                if is_streamable_content(&content) {
                                    let _ = tx.send(ChatEvent::Delta { content }).await;
                                }
                            }
                        }

                        // Accumulate tool calls and send ToolPlanning as names are detected
                        if let Some(tc_chunks) = choice.delta.tool_calls {
                            for tc_chunk in tc_chunks {
                                while tool_calls.len() <= tc_chunk.index {
                                    tool_calls.push(ToolCall {
                                        id: String::new(),
                                        call_type: "function".into(),
                                        function: FunctionCall {
                                            name: String::new(),
                                            arguments: String::new(),
                                        },
                                    });
                                }

                                let tc = &mut tool_calls[tc_chunk.index];
                                if let Some(id) = tc_chunk.id {
                                    tc.id = id;
                                }
                                if let Some(func) = tc_chunk.function {
                                    if let Some(name) = func.name {
                                        tc.function.name = name;
                                    }
                                    if let Some(args) = func.arguments {
                                        tc.function.arguments.push_str(&args);
                                    }
                                }
                            }

                            // Check if we have new tool names to report
                            let current_names: Vec<String> = tool_calls
                                .iter()
                                .filter(|tc| !tc.function.name.is_empty())
                                .map(|tc| tc.function.name.clone())
                                .collect();

                            if current_names != last_tool_names && !current_names.is_empty() {
                                last_tool_names = current_names.clone();
                                let _ = tx
                                    .send(ChatEvent::ToolPlanning {
                                        tools: current_names,
                                    })
                                    .await;
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }

        let duration_ms = start_time.elapsed().as_millis() as u64;

        Ok(ChatResult {
            request_id,
            content: if full_content.is_empty() {
                None
            } else {
                Some(full_content)
            },
            reasoning_content: if full_reasoning.is_empty() {
                None
            } else {
                Some(full_reasoning)
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage,
            duration_ms,
        })
    }
}
