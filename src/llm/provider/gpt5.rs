// src/llm/provider/gpt5.rs
// GPT-5 Responses API implementation with streaming + tool calling + structured outputs

use super::{LlmProvider, Message, Response, TokenUsage};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures::stream::{Stream, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use std::any::Any;
use std::pin::Pin;
use std::time::Instant;
use tracing::{debug, error, warn};

/// SSE stream that properly buffers lines across byte chunks
struct SseStream<S> {
    inner: S,
    buffer: String,
}

impl<S> SseStream<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: String::new(),
        }
    }
}

impl<S, E> Stream for SseStream<S>
where
    S: Stream<Item = Result<bytes::Bytes, E>> + Unpin,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = Result<String>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;

        loop {
            // Check if we have a complete line in buffer
            if let Some(newline_pos) = self.buffer.find('\n') {
                let line = self.buffer[..newline_pos].to_string();
                self.buffer.drain(..=newline_pos);
                return Poll::Ready(Some(Ok(line)));
            }

            // Need more data
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    match String::from_utf8(bytes.to_vec()) {
                        Ok(text) => {
                            self.buffer.push_str(&text);
                            // Continue loop to check for complete lines
                        }
                        Err(e) => {
                            return Poll::Ready(Some(Err(anyhow!(
                                "Invalid UTF-8 in stream: {}",
                                e
                            ))));
                        }
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(anyhow!("Stream error: {}", e))));
                }
                Poll::Ready(None) => {
                    // Stream ended, return remaining buffer if non-empty
                    if !self.buffer.is_empty() {
                        let line = std::mem::take(&mut self.buffer);
                        return Poll::Ready(Some(Ok(line)));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Stream events from GPT-5 Responses API
#[derive(Debug, Clone)]
pub enum Gpt5StreamEvent {
    /// Text content delta
    TextDelta { delta: String },
    /// Reasoning content delta  
    ReasoningDelta { delta: String },
    /// Tool call started
    ToolCallStart { id: String, name: String },
    /// Tool call arguments delta
    ToolCallArgumentsDelta { id: String, delta: String },
    /// Tool call completed
    ToolCallComplete {
        id: String,
        name: String,
        arguments: Value,
    },
    /// Response completed
    Done {
        response_id: String,
        input_tokens: i64,
        output_tokens: i64,
        reasoning_tokens: i64,
    },
    /// Error occurred
    Error { message: String },
}

#[derive(Clone)]
pub struct Gpt5Provider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: usize,
    verbosity: String,
    reasoning: String,
}

impl Gpt5Provider {
    pub fn new(
        api_key: String,
        model: String,
        max_tokens: usize,
        verbosity: String,
        reasoning: String,
    ) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            max_tokens,
            verbosity: normalize_verbosity(&verbosity),
            reasoning: normalize_reasoning(&reasoning),
        }
    }

    /// Create a response with tools (non-streaming)
    /// Returns response_id for multi-turn tracking
    pub async fn create_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        previous_response_id: Option<String>,
    ) -> Result<Gpt5ToolResponse> {
        let start = Instant::now();

        let body = self.build_request(
            messages,
            system,
            tools,
            previous_response_id,
            false, // non-streaming
            None,  // no response_format
        );

        debug!("GPT-5 request: {}", serde_json::to_string_pretty(&body)?);

        let response = self
            .client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            error!("GPT-5 API error {}: {}", status, error_text);
            return Err(anyhow!("GPT-5 API error {}: {}", status, error_text));
        }

        let response_json: Value = response.json().await?;
        debug!("GPT-5 response: {}", serde_json::to_string_pretty(&response_json)?);

        // Extract response_id
        let response_id = response_json
            .get("id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| anyhow!("Missing response_id in GPT-5 response"))?
            .to_string();

        // Extract text content
        let text = self.extract_text(&response_json);

        // Extract tool calls
        let tool_calls = self.extract_tool_calls(&response_json);

        // Extract token usage
        let tokens = self.extract_tokens(&response_json);

        let latency = start.elapsed().as_millis() as i64;

        Ok(Gpt5ToolResponse {
            response_id,
            content: text,
            tool_calls,
            model: self.model.clone(),
            tokens,
            latency_ms: latency,
        })
    }

    /// Chat with strict JSON schema enforcement (for analyzers and structured outputs)
    /// This uses GPT-5's json_schema response_format with strict validation
    pub async fn chat_with_schema(
        &self,
        messages: Vec<Message>,
        system: String,
        schema_name: &str,
        schema: Value,
    ) -> Result<Response> {
        let start = Instant::now();
        
        // Build input array
        let mut input = vec![];
        for msg in messages {
            input.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }
        
        let body = json!({
            "model": self.model,
            "input": input,
            "instructions": system,
            "max_output_tokens": self.max_tokens,
            "text": {
                "verbosity": self.verbosity,
                "format": {
                    "type": "json_schema",
                    "name": schema_name,
                    "schema": schema,
                    "strict": true
                }
            },
            "reasoning": {
                "effort": self.reasoning,
                "summary": "auto"
            },
            "store": false
        });
        
        debug!("GPT-5 structured request: {}", serde_json::to_string_pretty(&body)?);
        
        let response = self
            .client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        
        let status = response.status();
        let body_text = response.text().await?;
        
        if !status.is_success() {
            error!("GPT-5 schema error {}: {}", status, body_text);
            return Err(anyhow!("GPT-5 error {}: {}", status, body_text));
        }
        
        let json: Value = serde_json::from_str(&body_text)?;
        
        // Extract content from GPT-5 response
        // Structure: output[] -> type="message" -> content[] -> type="output_text" -> text
        let content = if let Some(output) = json.get("output").and_then(|o| o.as_array()) {
            debug!("Extracting from output array");
            let mut extracted = String::new();
            for item in output {
                if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                    if let Some(content_arr) = item.get("content").and_then(|c| c.as_array()) {
                        for part in content_arr {
                            // FIX: Changed from "text" to "output_text"
                            if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                                    extracted.push_str(t);
                                }
                            }
                        }
                    }
                }
            }
            extracted
        } else {
            String::new()
        };
        
        if content.is_empty() {
            error!("Failed to extract content from response");
            return Err(anyhow!("No content in structured response"));
        }
        
        let latency = start.elapsed().as_millis() as i64;
        
        Ok(Response {
            content,
            model: self.model.clone(),
            tokens: self.extract_tokens(&json),
            latency_ms: latency,
        })
    }

    /// Create a streaming response with tools
    /// Returns a stream of events + final response_id
    pub async fn create_stream_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        previous_response_id: Option<String>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Gpt5StreamEvent>> + Send>>> {
        let body = self.build_request(
            messages,
            system,
            tools,
            previous_response_id,
            true, // streaming
            None, // no response_format
        );

        debug!("GPT-5 streaming request: {}", serde_json::to_string_pretty(&body)?);

        let response = self
            .client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            error!("GPT-5 API streaming error {}: {}", status, error_text);
            return Err(anyhow!("GPT-5 API streaming error {}: {}", status, error_text));
        }

        // Create buffered SSE stream that properly handles line boundaries
        let stream = SseStream::new(response.bytes_stream());

        Ok(Box::pin(stream.filter_map(|result| async move {
            match result {
                Ok(line) => {
                    if line.is_empty() || !line.starts_with("data: ") {
                        return None;
                    }

                    let data = &line[6..]; // Skip "data: "

                    if data == "[DONE]" {
                        return None;
                    }

                    match serde_json::from_str::<Value>(data) {
                        Ok(json) => parse_sse_event(&json),
                        Err(e) => {
                            warn!("Failed to parse GPT-5 SSE: {} - Line: {}", e, data);
                            None
                        }
                    }
                }
                Err(e) => Some(Err(anyhow!("Stream error: {}", e))),
            }
        })))
    }

    /// Build request body for Responses API
    fn build_request(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        previous_response_id: Option<String>,
        stream: bool,
        response_format: Option<Value>,
    ) -> Value {
        // Format input messages
        let mut input = vec![json!({
            "role": "system",
            "content": system
        })];

        for msg in messages {
            input.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }

        let mut body = json!({
            "model": self.model,
            "input": input,
            "max_output_tokens": self.max_tokens,
            "text": {
                "verbosity": self.verbosity
            },
            "reasoning": {
                "effort": self.reasoning,
                "summary": "auto"
            },
            "store": true, // Store responses for multi-turn tracking
            "stream": stream,
        });

        // Add response_format if provided
        if let Some(format) = response_format {
            body["response_format"] = format;
        }

        // Add tools if present
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
            body["tool_choice"] = json!({"type": "auto"});
        }

        // Multi-turn: link to previous response
        if let Some(prev_id) = previous_response_id {
            body["previous_response_id"] = json!(prev_id);
            debug!("GPT-5 multi-turn: continuing from {}", prev_id);
        }

        body
    }

    /// Extract text from response
    fn extract_text(&self, response: &Value) -> String {
        // Try output_text first (convenience field)
        if let Some(text) = response.get("output_text").and_then(|t| t.as_str()) {
            return text.to_string();
        }

        // Fall back to output array
        // Structure: output[].type="message" -> content[].type="output_text" -> text
        if let Some(output) = response.get("output").and_then(|o| o.as_array()) {
            let mut text = String::new();
            for item in output {
                if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                    if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                        for part in content {
                            if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                                    text.push_str(t);
                                }
                            }
                        }
                    }
                }
            }
            return text;
        }

        String::new()
    }

    /// Extract tool calls from response
    fn extract_tool_calls(&self, response: &Value) -> Vec<ToolCall> {
        let mut calls = Vec::new();

        if let Some(output) = response.get("output").and_then(|o| o.as_array()) {
            for item in output {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    if let (Some(id), Some(name), Some(args)) = (
                        item.get("call_id").and_then(|i| i.as_str()),
                        item.get("name").and_then(|n| n.as_str()),
                        item.get("arguments"),
                    ) {
                        calls.push(ToolCall {
                            id: id.to_string(),
                            name: name.to_string(),
                            arguments: args.clone(),
                        });
                    }
                }
            }
        }

        calls
    }

    /// Extract token usage
    fn extract_tokens(&self, response: &Value) -> TokenUsage {
        let usage = response.get("usage");
        TokenUsage {
            input: usage
                .and_then(|u| u.get("input_tokens"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
            output: usage
                .and_then(|u| u.get("output_tokens"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
            reasoning: usage
                .and_then(|u| u.get("output_tokens_details"))
                .and_then(|d| d.get("reasoning_tokens"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0),
            cached: 0,
        }
    }
}

/// Response from GPT-5 with tool calling
#[derive(Debug, Clone)]
pub struct Gpt5ToolResponse {
    pub response_id: String,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub model: String,
    pub tokens: TokenUsage,
    pub latency_ms: i64,
}

/// Tool call from GPT-5
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Parse SSE event from GPT-5
fn parse_sse_event(json: &Value) -> Option<Result<Gpt5StreamEvent>> {
    let event_type = json.get("type")?.as_str()?;

    match event_type {
        "response.output_text.delta" => {
            let delta = json.get("delta")?.as_str()?.to_string();
            Some(Ok(Gpt5StreamEvent::TextDelta { delta }))
        }

        "response.reasoning.delta" => {
            let delta = json.get("delta")?.as_str()?.to_string();
            Some(Ok(Gpt5StreamEvent::ReasoningDelta { delta }))
        }

        "response.function_call_arguments.delta" => {
            let id = json.get("call_id")?.as_str()?.to_string();
            let delta = json.get("delta")?.as_str()?.to_string();
            Some(Ok(Gpt5StreamEvent::ToolCallArgumentsDelta { id, delta }))
        }

        "response.function_call_arguments.done" => {
            let id = json.get("call_id")?.as_str()?.to_string();
            let name = json.get("name")?.as_str()?.to_string();
            let arguments = json.get("arguments")?.clone();
            Some(Ok(Gpt5StreamEvent::ToolCallComplete {
                id,
                name,
                arguments,
            }))
        }

        "response.completed" | "response.done" => {
            let response_id = json.get("id")?.as_str()?.to_string();
            let usage = json.get("usage")?;

            let input_tokens = usage.get("input_tokens")?.as_i64().unwrap_or(0);
            let output_tokens = usage.get("output_tokens")?.as_i64().unwrap_or(0);
            let reasoning_tokens = usage
                .get("output_tokens_details")
                .and_then(|d| d.get("reasoning_tokens"))
                .and_then(|t| t.as_i64())
                .unwrap_or(0);

            Some(Ok(Gpt5StreamEvent::Done {
                response_id,
                input_tokens,
                output_tokens,
                reasoning_tokens,
            }))
        }

        "response.error" | "error" => {
            let message = json
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            Some(Ok(Gpt5StreamEvent::Error { message }))
        }

        // Ignore other event types
        _ => None,
    }
}

/// Normalize verbosity values (public for testing)
pub fn normalize_verbosity(v: &str) -> String {
    match v.to_lowercase().as_str() {
        "low" | "minimal" | "concise" => "low".to_string(),
        "high" | "detailed" | "verbose" => "high".to_string(),
        _ => "medium".to_string(),
    }
}

/// Normalize reasoning values (public for testing)
pub fn normalize_reasoning(r: &str) -> String {
    match r.to_lowercase().as_str() {
        "minimal" | "quick" => "low".to_string(),
        "high" | "thorough" | "deep" => "high".to_string(),
        _ => "medium".to_string(),
    }
}

#[async_trait]
impl LlmProvider for Gpt5Provider {
    fn name(&self) -> &'static str {
        "gpt-5"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn chat(&self, messages: Vec<Message>, system: String) -> Result<Response> {
        // Simple chat without tools
        let response = self
            .create_with_tools(messages, system, vec![], None)
            .await?;

        Ok(Response {
            content: response.content,
            model: response.model,
            tokens: response.tokens,
            latency_ms: response.latency_ms,
        })
    }

    // Note: chat_with_tools is defined in the LlmProvider trait
    // We don't implement it here since we have create_with_tools which is more specific
    async fn chat_with_tools(
        &self,
        _messages: Vec<Message>,
        _system: String,
        _tools: Vec<Value>,
        _context: Option<super::ToolContext>,
    ) -> Result<super::ToolResponse> {
        Err(anyhow!("Use create_with_tools or create_stream_with_tools instead"))
    }
}
