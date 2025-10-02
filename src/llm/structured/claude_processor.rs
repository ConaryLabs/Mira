// src/llm/structured/claude_processor.rs
// Claude Messages API processor with extended thinking support and tool calling

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tracing::{debug, info, error};
use crate::config::CONFIG;
use super::types::{LLMMetadata, StructuredLLMResponse};
use super::tool_schema::{get_response_tool_schema, get_code_fix_tool_schema};

/// Build Claude Messages API request with extended thinking
pub fn build_claude_request(
    user_message: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
    // Determine thinking budget and temperature based on message complexity
    let (thinking_budget, temperature) = analyze_message_complexity(user_message);
    
    debug!(
        "Claude request params: thinking={}, temp={}, context_msgs={}",
        thinking_budget, temperature, context_messages.len()
    );

    // Build messages array (Claude format)
    let mut messages = context_messages;
    messages.push(json!({
        "role": "user",
        "content": user_message
    }));

    Ok(json!({
        "model": CONFIG.anthropic_model,
        "max_tokens": CONFIG.anthropic_max_tokens,
        "temperature": temperature,
        "system": system_prompt,
        "messages": messages,
        "thinking": {
            "type": "enabled",
            "budget_tokens": thinking_budget
        }
    }))
}

/// Build request with forced tool choice for structured output
/// NOTE: Thinking is disabled because Claude doesn't allow it with forced tool use
pub fn build_claude_request_with_tool(
    user_message: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
    let (_thinking_budget, temperature) = analyze_message_complexity(user_message);
    
    debug!(
        "Claude tool request: temp={} (thinking disabled due to forced tool use)",
        temperature
    );

    let mut messages = context_messages;
    messages.push(json!({
        "role": "user",
        "content": user_message
    }));

    let request = json!({
        "model": CONFIG.anthropic_model,
        "max_tokens": CONFIG.anthropic_max_tokens,
        "temperature": temperature,
        "system": system_prompt,
        "messages": messages,
        // NO THINKING BLOCK - Claude API doesn't allow thinking with forced tool use
        "tools": [get_response_tool_schema()],
        "tool_choice": {
            "type": "tool",
            "name": "respond_to_user"
        }
    });
    
    // DEBUG LOGGING - Remove after verification
    error!("🔧 TOOL-BASED REQUEST BUILD:");
    error!("   Model: {}", CONFIG.anthropic_model);
    error!("   Temperature: {}", temperature);
    error!("   Thinking: DISABLED (forced tool use)");
    error!("   Tools present: {}", request["tools"].is_array());
    if let Some(tool_choice) = request["tool_choice"].as_object() {
        error!("   Tool choice name: {:?}", tool_choice.get("name"));
    }
    
    Ok(request)
}

/// Build request with custom function tools available
pub fn build_claude_request_with_custom_tools(
    user_message: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
    tool_schemas: Vec<Value>,
) -> Result<Value> {
    let (thinking_budget, temperature) = analyze_message_complexity(user_message);
    
    debug!(
        "Claude with {} custom tools: thinking={}, temp={}",
        tool_schemas.len(), thinking_budget, temperature
    );

    let mut messages = context_messages;
    messages.push(json!({
        "role": "user",
        "content": user_message
    }));

    Ok(json!({
        "model": CONFIG.anthropic_model,
        "max_tokens": CONFIG.anthropic_max_tokens,
        "temperature": temperature,
        "system": system_prompt,
        "messages": messages,
        "thinking": {
            "type": "enabled",
            "budget_tokens": thinking_budget
        },
        "tools": tool_schemas,
        // No tool_choice - Claude decides when to use tools (allows thinking)
    }))
}

/// Extract metadata from Claude response
pub fn extract_claude_metadata(raw_response: &Value, latency_ms: i64) -> Result<LLMMetadata> {
    let usage = raw_response["usage"].as_object()
        .ok_or_else(|| anyhow!("Missing usage in Claude response"))?;
    
    let input_tokens = usage["input_tokens"].as_i64().unwrap_or(0);
    let output_tokens = usage["output_tokens"].as_i64().unwrap_or(0);
    
    // Claude's thinking tokens (extended thinking feature)
    let thinking_tokens = usage.get("thinking_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    
    let total_tokens = input_tokens + output_tokens + thinking_tokens;
    
    let stop_reason = raw_response["stop_reason"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    Ok(LLMMetadata {
        response_id: raw_response["id"].as_str().map(|s| s.to_string()),
        prompt_tokens: Some(input_tokens),
        completion_tokens: Some(output_tokens),
        thinking_tokens: Some(thinking_tokens),
        total_tokens: Some(total_tokens),
        finish_reason: Some(stop_reason.clone()),
        latency_ms,
        model_version: raw_response["model"]
            .as_str()
            .unwrap_or(&CONFIG.anthropic_model)
            .to_string(),
        temperature: 0.7,  // Will be set correctly by caller
        max_tokens: CONFIG.anthropic_max_tokens as i64,
    })
}

/// Extract structured content from Claude response
pub fn extract_claude_content(raw_response: &Value) -> Result<StructuredLLMResponse> {
    let content = raw_response["content"].as_array()
        .ok_or_else(|| anyhow!("Missing content array in Claude response"))?;
    
    // Log thinking blocks if present (for debugging)
    for (i, block) in content.iter().enumerate() {
        if block["type"] == "thinking" {
            if let Some(thought) = block["thinking"].as_str() {
                info!("  Thought {}: {}", i + 1, thought);
            }
        }
    }
    
    // Find text content block
    let text_content = content.iter()
        .find(|block| block["type"] == "text")
        .and_then(|block| block["text"].as_str())
        .ok_or_else(|| anyhow!("No text content in Claude response"))?;
    
    // Parse JSON from text content
    let structured: StructuredLLMResponse = serde_json::from_str(text_content)
        .map_err(|e| anyhow!("Failed to parse structured response as JSON: {}", e))?;
    
    Ok(structured)
}

/// Extract from tool_use content block
pub fn extract_claude_content_from_tool(raw_response: &Value) -> Result<StructuredLLMResponse> {
    // DEBUG LOGGING - Remove after verification
    error!("🔍 RESPONSE EXTRACTION:");
    error!("   Raw response keys: {:?}", raw_response.as_object().map(|o| o.keys().collect::<Vec<_>>()));
    
    if let Some(content) = raw_response["content"].as_array() {
        error!("   Content array length: {}", content.len());
        for (i, block) in content.iter().enumerate() {
            let block_type = block["type"].as_str().unwrap_or("unknown");
            error!("   Block {}: type={}", i, block_type);
            if block_type == "tool_use" {
                error!("      Tool name: {:?}", block["name"]);
            }
        }
    } else {
        error!("   ❌ NO CONTENT ARRAY - this is the problem!");
        error!("   Full response: {}", serde_json::to_string_pretty(&raw_response).unwrap_or_default());
    }
    
    let content = raw_response["content"].as_array()
        .ok_or_else(|| anyhow!("Missing content array"))?;
    
    // Log thinking if present (won't be present with forced tool use, but kept for custom tools)
    for (i, block) in content.iter().enumerate() {
        if block["type"] == "thinking" {
            if let Some(thought) = block["thinking"].as_str() {
                info!("  Thought {}: {}", i + 1, thought);
            }
        }
    }
    
    // Find our tool call
    let tool_block = content.iter()
        .find(|block| {
            block["type"] == "tool_use" && 
            block["name"] == "respond_to_user"
        })
        .ok_or_else(|| anyhow!("No respond_to_user tool call"))?;
    
    let tool_input = &tool_block["input"];
    
    let structured: StructuredLLMResponse = serde_json::from_value(tool_input.clone())
        .map_err(|e| anyhow!("Failed to parse tool input: {}", e))?;
    
    Ok(structured)
}

/// Extract custom tool calls from response
pub fn extract_tool_calls(raw_response: &Value) -> Result<Vec<Value>> {
    let content = raw_response["content"].as_array()
        .ok_or_else(|| anyhow!("Missing content array"))?;
    
    let mut tool_calls = Vec::new();
    
    for block in content.iter() {
        if block["type"] == "tool_use" {
            tool_calls.push(block.clone());
        }
    }
    
    Ok(tool_calls)
}

/// Check if response contains tool calls
pub fn has_tool_calls(raw_response: &Value) -> bool {
    if let Some(content) = raw_response["content"].as_array() {
        return content.iter().any(|block| block["type"] == "tool_use");
    }
    false
}

/// Analyze message complexity to determine thinking budget and temperature
pub fn analyze_message_complexity(message: &str) -> (usize, f32) {
    let message_lower = message.to_lowercase();
    
    // Ultra-complex: architecture, refactoring, migration
    if message_lower.contains("refactor") 
        || message_lower.contains("architect") 
        || message_lower.contains("migrate")
        || message_lower.contains("redesign")
        || message.len() > 2000 {
        return (CONFIG.thinking_budget_ultra, CONFIG.temperature_balanced);
    }
    
    // Complex: debugging, optimization, complex logic, error fixes
    if message_lower.contains("debug") 
        || message_lower.contains("optimize")
        || message_lower.contains("fix error")
        || message_lower.contains("compiler error")
        || message_lower.contains("error[")  // Rust error format
        || message.len() > 1000 {
        return (CONFIG.thinking_budget_complex, CONFIG.temperature_deterministic);
    }
    
    // Default: standard coding tasks
    if message_lower.contains("implement") 
        || message_lower.contains("write")
        || message_lower.contains("create")
        || message.len() > 300 {
        return (CONFIG.thinking_budget_default, CONFIG.temperature_balanced);
    }
    
    // Simple: quick questions, explanations
    (CONFIG.thinking_budget_simple, CONFIG.temperature_balanced)
}
