// src/llm/client/streaming.rs
// Fixed version with corrected API endpoint

use std::pin::Pin;

use anyhow::Result;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;
use tracing::{debug, warn};
use reqwest::{header, Client};

use crate::llm::client::config::ClientConfig;

/// Stream of JSON payloads from the OpenAI Responses SSE.
pub type ResponseStream = Pin<Box<dyn Stream<Item = Result<Value>> + Send>>;

/// Create SSE stream for streaming responses
pub async fn create_sse_stream(
    client: &Client,
    config: &ClientConfig,
    body: Value,
) -> Result<ResponseStream> {
    let req = client
        .post(format!("{}/v1/responses", config.base_url()))  // FIXED: removed /openai prefix
        .header(header::AUTHORIZATION, format!("Bearer {}", config.api_key()))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .json(&body);

    let resp = req.send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let error_text = resp.text().await.unwrap_or_else(|_| "<no body>".into());
        return Err(anyhow::anyhow!("OpenAI API error ({}): {}", status, error_text));
    }

    let bytes_stream = resp.bytes_stream();
    let stream = sse_json_stream(bytes_stream);
    Ok(Box::pin(stream))
}

/// Parse SSE stream of JSON into a Stream of Value.
/// Filters out empty lines, "data: " prefixes, and parses JSON chunks.
pub fn sse_json_stream(
    bytes_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<Value>> + Send {
    bytes_stream
        .map(|chunk_result| {
            chunk_result
                .map_err(|e| anyhow::anyhow!("Stream error: {}", e))
                .and_then(|chunk| {
                    let text = String::from_utf8_lossy(&chunk);
                    parse_sse_chunk(&text)
                })
        })
        .filter_map(|result| async move {
            match result {
                Ok(Some(value)) => Some(Ok(value)),
                Ok(None) => None,  // Skip empty chunks
                Err(e) => Some(Err(e)),
            }
        })
}

/// Parse a single SSE chunk into JSON Value
pub fn parse_sse_chunk(chunk_text: &str) -> Result<Option<Value>> {
    for line in chunk_text.lines() {
        let line = line.trim();
        
        // Skip empty lines and comments
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        
        // Handle SSE event types
        if line.starts_with("event:") {
            let event_type = line.strip_prefix("event:").unwrap_or("").trim();
            debug!("SSE event type: {}", event_type);
            continue;
        }
        
        // Handle data lines
        if line.starts_with("data:") {
            let data_part = line.strip_prefix("data:").unwrap_or("").trim();
            
            // Check for stream end marker
            if data_part == "[DONE]" {
                debug!("Stream completed: [DONE] marker received");
                return Ok(None);
            }
            
            // Skip empty data
            if data_part.is_empty() {
                continue;
            }
            
            // Try to parse as JSON
            match serde_json::from_str::<Value>(data_part) {
                Ok(json_value) => {
                    debug!("Parsed SSE JSON chunk: {}", 
                           serde_json::to_string(&json_value).unwrap_or_default());
                    return Ok(Some(json_value));
                }
                Err(e) => {
                    warn!("Failed to parse SSE JSON: {} - Data: {}", e, data_part);
                    continue;
                }
            }
        }
    }
    
    // No valid JSON found in this chunk
    Ok(None)
}

/// Extract content delta from streaming chunk
pub fn extract_content_from_chunk(chunk: &Value) -> Option<String> {
    // Try different paths for content extraction
    
    // 1) Standard delta format: choices[0].delta.content
    if let Some(content) = chunk.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            debug!("Extracted delta content: {} chars", content.len());
            return Some(content.to_string());
        }
    }
    
    // 2) Response API format: output[0].content[0].text
    if let Some(content) = chunk.pointer("/output/0/content/0/text").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            debug!("Extracted response API content: {} chars", content.len());
            return Some(content.to_string());
        }
    }
    
    // 3) Delta text format
    if let Some(content) = chunk.pointer("/delta/text").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            debug!("Extracted delta text: {} chars", content.len());
            return Some(content.to_string());
        }
    }
    
    // 4) Direct content field
    if let Some(content) = chunk.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            debug!("Extracted direct content: {} chars", content.len());
            return Some(content.to_string());
        }
    }
    
    // 5) Text field
    if let Some(content) = chunk.get("text").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            debug!("Extracted text content: {} chars", content.len());
            return Some(content.to_string());
        }
    }
    
    None
}

/// Check if streaming chunk indicates completion
pub fn is_completion_chunk(chunk: &Value) -> bool {
    // Check for finish reasons
    if let Some(finish_reason) = chunk.pointer("/choices/0/finish_reason") {
        return !finish_reason.is_null();
    }
    
    // Check for completion signals in response API format
    if let Some(event_type) = chunk.get("type").and_then(|t| t.as_str()) {
        return event_type == "response.done" || event_type == "response.completed";
    }
    
    false
}

/// Tool call delta for streaming
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCallDelta {
    pub index: Option<usize>,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub tool_type: Option<String>,
    pub function: Option<FunctionDelta>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

/// Usage information for streaming
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StreamingUsage {
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub reasoning_tokens: Option<i32>,
}

/// Streaming response processor
pub struct StreamProcessor {
    content_buffer: String,
    tool_calls_buffer: Vec<ToolCallDelta>,
    usage_info: Option<StreamingUsage>,
}

impl StreamProcessor {
    pub fn new() -> Self {
        Self {
            content_buffer: String::new(),
            tool_calls_buffer: Vec::new(),
            usage_info: None,
        }
    }
    
    /// Process a streaming chunk and update internal state
    pub fn process_chunk(&mut self, chunk: &Value) -> Option<StreamingEvent> {
        // Extract content delta
        if let Some(content) = extract_content_from_chunk(chunk) {
            self.content_buffer.push_str(&content);
            return Some(StreamingEvent::ContentDelta(content));
        }
        
        // Check for tool call deltas
        if let Some(tool_calls) = chunk.pointer("/choices/0/delta/tool_calls") {
            if let Some(calls) = tool_calls.as_array() {
                for call in calls {
                    if let Ok(tool_call) = serde_json::from_value::<ToolCallDelta>(call.clone()) {
                        self.tool_calls_buffer.push(tool_call.clone());
                        return Some(StreamingEvent::ToolCallDelta(tool_call));
                    }
                }
            }
        }
        
        // Check for usage information
        if let Some(usage) = chunk.get("usage") {
            if let Ok(usage_info) = serde_json::from_value::<StreamingUsage>(usage.clone()) {
                self.usage_info = Some(usage_info.clone());
                return Some(StreamingEvent::Usage(usage_info));
            }
        }
        
        // Check for completion
        if is_completion_chunk(chunk) {
            return Some(StreamingEvent::Complete {
                content: self.content_buffer.clone(),
                tool_calls: self.tool_calls_buffer.clone(),
                usage: self.usage_info.clone(),
            });
        }
        
        None
    }
    
    /// Get accumulated content
    pub fn get_content(&self) -> &str {
        &self.content_buffer
    }
    
    /// Get accumulated tool calls
    pub fn get_tool_calls(&self) -> &[ToolCallDelta] {
        &self.tool_calls_buffer
    }
}

/// Events emitted during streaming
#[derive(Debug, Clone)]
pub enum StreamingEvent {
    ContentDelta(String),
    ToolCallDelta(ToolCallDelta),
    Usage(StreamingUsage),
    Complete {
        content: String,
        tool_calls: Vec<ToolCallDelta>,
        usage: Option<StreamingUsage>,
    },
}
