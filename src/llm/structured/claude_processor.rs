// src/llm/structured/claude_processor.rs
// Claude Messages API processor with extended thinking support

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tracing::{debug, info};
use crate::config::CONFIG;
use super::types::{LLMMetadata, StructuredLLMResponse};

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

/// Analyze message complexity to determine thinking budget and temperature
fn analyze_message_complexity(message: &str) -> (usize, f32) {
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
