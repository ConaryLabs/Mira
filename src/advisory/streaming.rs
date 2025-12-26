//! Streaming Module - SSE parsing and streaming orchestration
//!
//! Handles server-sent events (SSE) parsing for different providers
//! and streaming council orchestration with timeouts.

#![allow(dead_code)]

use anyhow::Result;
use futures::StreamExt;
use reqwest::Response;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::{AdvisoryModel, AdvisoryRequest, AdvisoryProvider, AdvisoryEvent};

/// Default timeout per provider (60 seconds)
pub const DEFAULT_STREAM_TIMEOUT: Duration = Duration::from_secs(60);

/// Extended timeout for reasoning models (180 seconds)
pub const REASONER_STREAM_TIMEOUT: Duration = Duration::from_secs(180);

/// Result of a streaming council query
#[derive(Debug, Clone)]
pub struct StreamingCouncilResult {
    /// Completed responses (model -> final text)
    pub responses: HashMap<AdvisoryModel, String>,
    /// Models that timed out
    pub timeouts: Vec<AdvisoryModel>,
    /// Models that errored
    pub errors: HashMap<AdvisoryModel, String>,
}

impl StreamingCouncilResult {
    pub fn new() -> Self {
        Self {
            responses: HashMap::new(),
            timeouts: vec![],
            errors: HashMap::new(),
        }
    }

    /// Check if we have at least one successful response
    pub fn has_responses(&self) -> bool {
        !self.responses.is_empty()
    }

    /// Get count of successful responses
    pub fn success_count(&self) -> usize {
        self.responses.len()
    }
}

// ============================================================================
// OpenAI Streaming
// ============================================================================

#[derive(Deserialize, Debug)]
struct OpenAIStreamChunk {
    choices: Option<Vec<OpenAIStreamChoice>>,
}

#[derive(Deserialize, Debug)]
struct OpenAIStreamChoice {
    delta: Option<OpenAIStreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct OpenAIStreamDelta {
    content: Option<String>,
}

/// Parse OpenAI SSE stream
pub async fn parse_openai_stream(
    response: Response,
    tx: mpsc::Sender<AdvisoryEvent>,
) -> Result<String> {
    let mut full_text = String::new();
    let mut stream = response.bytes_stream();

    let mut buffer = String::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete SSE lines
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() || line == "data: [DONE]" {
                continue;
            }

            if let Some(json_str) = line.strip_prefix("data: ") {
                if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(json_str) {
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
// Anthropic Streaming
// ============================================================================

#[derive(Deserialize, Debug)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<AnthropicDelta>,
    content_block: Option<AnthropicContentBlock>,
}

#[derive(Deserialize, Debug)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
}

/// Parse Anthropic SSE stream
pub async fn parse_anthropic_stream(
    response: Response,
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
                if let Ok(event) = serde_json::from_str::<AnthropicStreamEvent>(json_str) {
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
// Gemini Streaming
// ============================================================================

#[derive(Deserialize, Debug)]
struct GeminiStreamChunk {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Deserialize, Debug)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Deserialize, Debug)]
struct GeminiContent {
    parts: Option<Vec<GeminiPart>>,
}

#[derive(Deserialize, Debug)]
struct GeminiPart {
    text: Option<String>,
}

/// Parse Gemini SSE stream
pub async fn parse_gemini_stream(
    response: Response,
    tx: mpsc::Sender<AdvisoryEvent>,
) -> Result<String> {
    let mut full_text = String::new();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Gemini sends JSON array chunks, look for complete objects
        while let Some(obj_start) = buffer.find('{') {
            // Find matching closing brace
            let mut depth = 0;
            let mut obj_end = None;
            for (i, c) in buffer[obj_start..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            obj_end = Some(obj_start + i + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if let Some(end) = obj_end {
                let json_str = &buffer[obj_start..end];
                if let Ok(chunk) = serde_json::from_str::<GeminiStreamChunk>(json_str) {
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
                buffer = buffer[end..].to_string();
            } else {
                break; // Incomplete object, wait for more data
            }
        }
    }

    let _ = tx.send(AdvisoryEvent::Done).await;
    Ok(full_text)
}

// ============================================================================
// DeepSeek Streaming
// ============================================================================

#[derive(Deserialize, Debug)]
struct DeepSeekStreamChunk {
    choices: Option<Vec<DeepSeekStreamChoice>>,
}

#[derive(Deserialize, Debug)]
struct DeepSeekStreamChoice {
    delta: Option<DeepSeekStreamDelta>,
}

#[derive(Deserialize, Debug)]
struct DeepSeekStreamDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
}

/// Parse DeepSeek SSE stream
pub async fn parse_deepseek_stream(
    response: Response,
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
                if let Ok(chunk) = serde_json::from_str::<DeepSeekStreamChunk>(json_str) {
                    if let Some(choices) = chunk.choices {
                        for choice in choices {
                            if let Some(delta) = choice.delta {
                                // Send reasoning content as separate event
                                if let Some(reasoning) = delta.reasoning_content {
                                    let _ = tx.send(AdvisoryEvent::ReasoningDelta(reasoning)).await;
                                }
                                // Send regular content
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
// Council Streaming Orchestration
// ============================================================================

/// Stream from multiple providers in parallel with timeout handling
pub async fn stream_council(
    providers: Vec<(AdvisoryModel, Arc<dyn AdvisoryProvider>)>,
    request: AdvisoryRequest,
    per_provider_timeout: Duration,
) -> StreamingCouncilResult {
    let mut result = StreamingCouncilResult::new();

    // Create futures for all providers
    let futures: Vec<_> = providers
        .into_iter()
        .map(|(model, provider)| {
            let req = request.clone();
            async move {
                let stream_result = timeout(
                    per_provider_timeout,
                    provider.complete(req),
                ).await;

                match stream_result {
                    Ok(Ok(response)) => (model, Ok(response.text)),
                    Ok(Err(e)) => (model, Err(format!("Error: {}", e))),
                    Err(_) => (model, Err("Timeout".to_string())),
                }
            }
        })
        .collect();

    // Run all in parallel
    let results = futures::future::join_all(futures).await;

    // Collect results
    for (model, res) in results {
        match res {
            Ok(text) => {
                result.responses.insert(model, text);
            }
            Err(e) if e == "Timeout" => {
                result.timeouts.push(model);
            }
            Err(e) => {
                result.errors.insert(model, e);
            }
        }
    }

    result
}

/// Progress update during council streaming
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CouncilProgress {
    // === Single-round events ===
    /// A model has started responding
    ModelStarted { model: String },
    /// A model sent a text delta
    ModelDelta { model: String, delta: String },
    /// A model completed
    ModelCompleted {
        model: String,
        text: String,
        /// Reasoning snippet (first ~500 chars) for UI timeline
        reasoning_snippet: Option<String>,
    },
    /// A model timed out
    ModelTimeout { model: String },
    /// A model errored
    ModelError { model: String, error: String },
    /// Synthesis has started
    SynthesisStarted,
    /// Synthesis delta
    SynthesisDelta { delta: String },
    /// All done (single-round)
    Done { result: serde_json::Value },

    // === Tool calling events ===
    /// A model is calling a tool
    ModelToolCall {
        model: String,
        tool_name: String,
        round: u8,
    },
    /// A model's tool call completed
    ModelToolResult {
        model: String,
        tool_name: String,
        success: bool,
        round: u8,
    },
    /// Model finished all tool calls for this round
    ModelToolsComplete {
        model: String,
        tools_called: Vec<String>,
        round: u8,
    },

    // === Multi-round deliberation events ===
    /// Deliberation round is starting
    RoundStarted { round: u8, max_rounds: u8 },
    /// Round complete, moderator is analyzing
    ModeratorAnalyzing { round: u8 },
    /// Moderator analysis complete
    ModeratorComplete {
        round: u8,
        should_continue: bool,
        disagreements: Vec<String>,
        focus_questions: Vec<String>,
        resolved_points: Vec<String>,
    },
    /// Early consensus reached
    EarlyConsensus { round: u8, reason: Option<String> },
    /// Deliberation complete with full result
    DeliberationComplete { result: serde_json::Value },
    /// Deliberation failed
    DeliberationFailed { error: String },
}

// ============================================================================
// Progress Sink Trait
// ============================================================================

/// Trait for sinking progress events during council deliberation.
///
/// This allows the core deliberation logic to emit progress events without
/// knowing whether they go to SSE streaming, nowhere (noop), or elsewhere.
#[async_trait::async_trait]
pub trait ProgressSink: Send + Sync {
    /// Emit a progress event. Implementations decide what to do with it.
    async fn emit(&self, event: CouncilProgress);
}

/// No-op sink that discards all events.
///
/// Used when only database progress updates are needed (MCP path).
pub struct NoopSink;

#[async_trait::async_trait]
impl ProgressSink for NoopSink {
    async fn emit(&self, _event: CouncilProgress) {
        // Intentionally empty - events are discarded
    }
}

/// SSE sink that sends events to a channel for HTTP streaming.
pub struct SseSink {
    tx: mpsc::Sender<CouncilProgress>,
}

impl SseSink {
    pub fn new(tx: mpsc::Sender<CouncilProgress>) -> Self {
        Self { tx }
    }
}

#[async_trait::async_trait]
impl ProgressSink for SseSink {
    async fn emit(&self, event: CouncilProgress) {
        // Ignore send errors - receiver may have dropped
        let _ = self.tx.send(event).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_council_result() {
        let mut result = StreamingCouncilResult::new();
        assert!(!result.has_responses());
        assert_eq!(result.success_count(), 0);

        result.responses.insert(AdvisoryModel::Gpt52, "test".to_string());
        assert!(result.has_responses());
        assert_eq!(result.success_count(), 1);

        result.timeouts.push(AdvisoryModel::Opus45);
        result.errors.insert(AdvisoryModel::Gemini3Pro, "error".to_string());
        assert_eq!(result.success_count(), 1);
    }
}
