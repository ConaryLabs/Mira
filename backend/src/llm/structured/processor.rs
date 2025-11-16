// src/llm/structured/processor.rs
// Response processing and extraction helpers

use anyhow::{Result, anyhow};
use serde_json::Value;

use super::types::*;
use crate::llm::provider::ToolResponse;

/// Check if response has tool calls
pub fn has_tool_calls(response: &ToolResponse) -> bool {
    !response.function_calls.is_empty()
}

/// Extract structured content from tool response (looks for respond_to_user tool)
pub fn extract_claude_content_from_tool(response: &ToolResponse) -> Result<StructuredLLMResponse> {
    let respond_call = response
        .function_calls
        .iter()
        .find(|fc| fc.name == "respond_to_user")
        .ok_or_else(|| anyhow!("No respond_to_user tool call found"))?;

    let structured: StructuredLLMResponse = serde_json::from_value(respond_call.arguments.clone())
        .map_err(|e| anyhow!("Failed to parse respond_to_user arguments: {}", e))?;

    Ok(structured)
}

/// Extract metadata from tool response
pub fn extract_claude_metadata(response: &ToolResponse, latency_ms: i64) -> Result<LLMMetadata> {
    Ok(LLMMetadata {
        response_id: Some(response.id.clone()),
        model_version: "provider".to_string(),
        prompt_tokens: Some(response.tokens.input),
        completion_tokens: Some(response.tokens.output),
        thinking_tokens: if response.tokens.reasoning > 0 {
            Some(response.tokens.reasoning)
        } else {
            None
        },
        total_tokens: Some(
            response.tokens.input + response.tokens.output + response.tokens.reasoning,
        ),
        latency_ms,
        finish_reason: Some("tool_use".to_string()),
        temperature: 0.7,
        max_tokens: 4096,
    })
}

/// Analyze message complexity to determine thinking budget and temperature
pub fn analyze_message_complexity(message: &str) -> (usize, f32) {
    let message_lower = message.to_lowercase();

    // Ultra-complex: architecture, refactoring, migration
    if message_lower.contains("refactor")
        || message_lower.contains("architect")
        || message_lower.contains("migrate")
        || message_lower.contains("redesign")
        || message.len() > 2000
    {
        return (50000, 0.7); // 50K budget, balanced temp
    }

    // Complex: debugging, optimization, complex logic, error fixes
    if message_lower.contains("debug")
        || message_lower.contains("optimize")
        || message_lower.contains("fix error")
        || message_lower.contains("compiler error")
        || message_lower.contains("error[")
        || message.len() > 1000
    {
        return (20000, 0.3); // 20K budget, deterministic temp
    }

    // Default: standard coding tasks
    if message_lower.contains("implement")
        || message_lower.contains("write")
        || message_lower.contains("create")
        || message.len() > 300
    {
        return (10000, 0.7); // 10K budget, balanced temp
    }

    // Simple: quick questions, explanations
    (5000, 0.7) // 5K budget, balanced temp
}

/// Extract metadata from raw Value response (legacy compatibility)
pub fn extract_metadata(raw_response: &Value, latency_ms: i64) -> Result<LLMMetadata> {
    let usage = &raw_response["usage"];

    Ok(LLMMetadata {
        response_id: raw_response["id"].as_str().map(String::from),
        model_version: raw_response["model"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        prompt_tokens: usage["input_tokens"].as_i64(),
        completion_tokens: usage["output_tokens"].as_i64(),
        thinking_tokens: usage["thinking_tokens"].as_i64(),
        total_tokens: Some(
            usage["input_tokens"].as_i64().unwrap_or(0)
                + usage["output_tokens"].as_i64().unwrap_or(0)
                + usage["thinking_tokens"].as_i64().unwrap_or(0),
        ),
        latency_ms,
        finish_reason: raw_response["stop_reason"].as_str().map(String::from),
        temperature: 0.7,
        max_tokens: 4096,
    })
}
