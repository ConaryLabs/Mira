// src/llm/client/streaming.rs
// Phase 3: Extract Streaming Logic from client.rs
// Handles SSE parsing, streaming response processing, and chunk extraction

use std::pin::Pin;

use anyhow::Result;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;
use tracing::{debug, warn};

/// Stream of JSON payloads from the OpenAI Responses SSE.
pub type ResponseStream = Pin<Box<dyn Stream<Item = Result<Value>> + Send>>;

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
            debug!("Extracted text field: {} chars", content.len());
            return Some(content.to_string());
        }
    }

    // No content found
    None
}

/// Extract streaming event type from chunk
pub fn extract_event_type(chunk: &Value) -> Option<String> {
    chunk.get("type")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // Check for event in object field
            chunk.get("object")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
}

/// Check if chunk indicates completion
pub fn is_completion_chunk(chunk: &Value) -> bool {
    // Check for finish_reason
    if let Some(finish_reason) = chunk.pointer("/choices/0/finish_reason") {
        return !finish_reason.is_null();
    }
    
    // Check for done flag
    if let Some(done) = chunk.get("done").and_then(|d| d.as_bool()) {
        return done;
    }
    
    // Check for completion event type
    if let Some(event_type) = extract_event_type(chunk) {
        return event_type == "completion" || event_type == "done";
    }
    
    false
}

/// Extract tool calls from streaming chunk
pub fn extract_tool_calls_from_chunk(chunk: &Value) -> Vec<ToolCallDelta> {
    let mut tool_calls = Vec::new();
    
    // Try different paths for tool calls
    if let Some(calls) = chunk.pointer("/choices/0/delta/tool_calls").and_then(|t| t.as_array()) {
        for call in calls {
            if let Ok(tool_call) = serde_json::from_value::<ToolCallDelta>(call.clone()) {
                tool_calls.push(tool_call);
            }
        }
    }
    
    // Try output format
    if let Some(output_array) = chunk.get("output").and_then(|o| o.as_array()) {
        for item in output_array {
            if item.get("type").and_then(|t| t.as_str()) == Some("tool_call") {
                if let Ok(tool_call) = serde_json::from_value::<ToolCallDelta>(item.clone()) {
                    tool_calls.push(tool_call);
                }
            }
        }
    }
    
    tool_calls
}

/// Extract usage information from final chunk
pub fn extract_usage_from_chunk(chunk: &Value) -> Option<StreamingUsage> {
    chunk.get("usage")
        .and_then(|u| serde_json::from_value(u.clone()).ok())
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
    pub fn process_chunk(&mut self, chunk: &Value) -> ProcessResult {
        let mut result = ProcessResult::default();
        
        // Extract content delta
        if let Some(content) = extract_content_from_chunk(chunk) {
            self.content_buffer.push_str(&content);
            result.content_delta = Some(content);
        }
        
        // Extract tool calls
        let tool_calls = extract_tool_calls_from_chunk(chunk);
        if !tool_calls.is_empty() {
            self.tool_calls_buffer.extend(tool_calls.clone());
            result.tool_calls = Some(tool_calls);
        }
        
        // Extract usage info
        if let Some(usage) = extract_usage_from_chunk(chunk) {
            self.usage_info = Some(usage.clone());
            result.usage = Some(usage);
        }
        
        // Check completion
        result.is_complete = is_completion_chunk(chunk);
        
        result
    }
    
    /// Get the accumulated content
    pub fn get_content(&self) -> &str {
        &self.content_buffer
    }
    
    /// Get the accumulated tool calls
    pub fn get_tool_calls(&self) -> &[ToolCallDelta] {
        &self.tool_calls_buffer
    }
    
    /// Get usage information
    pub fn get_usage(&self) -> Option<&StreamingUsage> {
        self.usage_info.as_ref()
    }
}

/// Result of processing a single chunk
#[derive(Debug, Default)]
pub struct ProcessResult {
    pub content_delta: Option<String>,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
    pub usage: Option<StreamingUsage>,
    pub is_complete: bool,
}

impl Default for StreamProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_sse_chunk() {
        let chunk = "data: {\"content\": \"Hello\"}\n\n";
        let result = parse_sse_chunk(chunk).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap()["content"], "Hello");
    }

    #[test]
    fn test_parse_sse_done() {
        let chunk = "data: [DONE]\n\n";
        let result = parse_sse_chunk(chunk).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_content_from_chunk() {
        let chunk = json!({
            "choices": [
                {
                    "delta": {
                        "content": "Hello, world!"
                    }
                }
            ]
        });

        let content = extract_content_from_chunk(&chunk);
        assert_eq!(content, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_extract_content_response_api() {
        let chunk = json!({
            "output": [
                {
                    "content": [
                        {
                            "text": "Response API content"
                        }
                    ]
                }
            ]
        });

        let content = extract_content_from_chunk(&chunk);
        assert_eq!(content, Some("Response API content".to_string()));
    }

    #[test]
    fn test_is_completion_chunk() {
        let completion = json!({
            "choices": [
                {
                    "finish_reason": "stop"
                }
            ]
        });

        assert!(is_completion_chunk(&completion));

        let not_done = json!({
            "choices": [
                {
                    "delta": {
                        "content": "more content"
                    }
                }
            ]
        });

        assert!(!is_completion_chunk(&not_done));
    }

    #[test]
    fn test_stream_processor() {
        let mut processor = StreamProcessor::new();
        
        let chunk1 = json!({
            "choices": [
                {
                    "delta": {
                        "content": "Hello"
                    }
                }
            ]
        });
        
        let result1 = processor.process_chunk(&chunk1);
        assert_eq!(result1.content_delta, Some("Hello".to_string()));
        assert!(!result1.is_complete);
        
        let chunk2 = json!({
            "choices": [
                {
                    "delta": {
                        "content": " world!"
                    }
                }
            ]
        });
        
        let result2 = processor.process_chunk(&chunk2);
        assert_eq!(result2.content_delta, Some(" world!".to_string()));
        
        assert_eq!(processor.get_content(), "Hello world!");
    }

    #[test]
    fn test_extract_event_type() {
        let chunk = json!({
            "type": "response.text.delta"
        });

        let event_type = extract_event_type(&chunk);
        assert_eq!(event_type, Some("response.text.delta".to_string()));
    }
}
