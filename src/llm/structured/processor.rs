// src/llm/structured/processor.rs
// Delegates to Claude processor for request/response handling with tool calling

use anyhow::Result;
use serde_json::Value;

use super::types::*;
use crate::llm::structured::{has_tool_calls, extract_claude_content_from_tool, extract_claude_metadata, analyze_message_complexity};

// Delegate to tool-based request building
pub fn build_structured_request(
    user_message: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
    build_claude_request_with_tool(
        user_message,
        system_prompt,
        context_messages,
    )
}

// Delegate metadata extraction to Claude processor
pub fn extract_metadata(raw_response: &Value, latency_ms: i64) -> Result<LLMMetadata> {
    extract_claude_metadata(raw_response, latency_ms)
}

// Delegate to tool-based content extraction
pub fn extract_structured_content(raw_response: &Value) -> Result<StructuredLLMResponse> {
    extract_claude_content_from_tool(raw_response)
}
