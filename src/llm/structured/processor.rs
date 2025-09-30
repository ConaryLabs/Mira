// src/llm/structured/processor.rs
// Delegates to Claude processor for request/response handling with tool calling

use anyhow::Result;
use serde_json::Value;

use super::types::*;
use super::claude_processor;  // Import from sibling module

// Delegate to tool-based request building
pub fn build_structured_request(
    user_message: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
    claude_processor::build_claude_request_with_tool(
        user_message,
        system_prompt,
        context_messages,
    )
}

// Delegate metadata extraction to Claude processor
pub fn extract_metadata(raw_response: &Value, latency_ms: i64) -> Result<LLMMetadata> {
    claude_processor::extract_claude_metadata(raw_response, latency_ms)
}

// Delegate to tool-based content extraction
pub fn extract_structured_content(raw_response: &Value) -> Result<StructuredLLMResponse> {
    claude_processor::extract_claude_content_from_tool(raw_response)
}
