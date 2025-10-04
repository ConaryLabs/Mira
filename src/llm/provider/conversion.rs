// src/llm/provider/conversion.rs
// Format conversion helpers for translating between provider formats

use super::{ChatMessage, ProviderResponse};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Extract messages from Claude-format request
pub fn extract_messages_from_claude_format(request: &Value) -> Result<Vec<ChatMessage>> {
    let messages_array = request["messages"]
        .as_array()
        .ok_or_else(|| anyhow!("No messages array in request"))?;
    
    let mut messages = Vec::new();
    for msg in messages_array {
        let role = msg["role"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing role in message"))?
            .to_string();
        
        let content = msg["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing content in message"))?
            .to_string();
        
        messages.push(ChatMessage { role, content });
    }
    
    Ok(messages)
}

/// Convert ProviderResponse to Claude-format Value for backward compatibility
pub fn provider_response_to_claude_format(response: ProviderResponse) -> Value {
    let mut claude_response = json!({
        "content": [{
            "type": "text",
            "text": response.content
        }],
        "stop_reason": response.metadata.finish_reason.unwrap_or_else(|| "end_turn".to_string()),
        "usage": {
            "input_tokens": response.metadata.input_tokens.unwrap_or(0),
            "output_tokens": response.metadata.output_tokens.unwrap_or(0),
        }
    });
    
    // Add thinking if present
    if let Some(thinking) = response.thinking {
        claude_response["thinking"] = json!(thinking);
        if let Some(thinking_tokens) = response.metadata.thinking_tokens {
            claude_response["usage"]["thinking_tokens"] = json!(thinking_tokens);
        }
    }
    
    claude_response
}

/// Convert GPT-5 tool response to Claude format
pub fn gpt5_tool_response_to_claude(response: &Value) -> Result<Value> {
    // GPT-5 Responses API tool format:
    // { "output": [{ "type": "function_call", "call_id": "...", "name": "...", "arguments": "..." }] }
    
    let output = response.get("output")
        .and_then(|o| o.as_array())
        .ok_or_else(|| anyhow!("No output in GPT-5 response"))?;
    
    let mut claude_content = Vec::new();
    
    for item in output {
        if item["type"] == "function_call" {
            // Convert to Claude tool_use format
            claude_content.push(json!({
                "type": "tool_use",
                "id": item["call_id"],
                "name": item["name"],
                "input": serde_json::from_str::<Value>(
                    item["arguments"].as_str().unwrap_or("{}")
                ).unwrap_or(json!({}))
            }));
        } else if item["type"] == "message" {
            // Regular text message
            if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                for c in content {
                    if c["type"] == "text" {
                        claude_content.push(json!({
                            "type": "text",
                            "text": c["text"]
                        }));
                    }
                }
            }
        }
    }
    
    // Extract usage
    let usage = response.get("usage");
    
    Ok(json!({
        "content": claude_content,
        "stop_reason": response["finish_reason"].as_str().unwrap_or("end_turn"),
        "usage": {
            "input_tokens": usage.and_then(|u| u["prompt_tokens"].as_i64()).unwrap_or(0),
            "output_tokens": usage.and_then(|u| u["completion_tokens"].as_i64()).unwrap_or(0),
        }
    }))
}

/// Convert DeepSeek tool response to Claude format
pub fn deepseek_tool_response_to_claude(response: &Value) -> Result<Value> {
    // DeepSeek uses OpenAI format:
    // { "choices": [{ "message": { "tool_calls": [...] } }] }
    
    let message = response["choices"][0]["message"]
        .as_object()
        .ok_or_else(|| anyhow!("No message in DeepSeek response"))?;
    
    let mut claude_content = Vec::new();
    
    // Add text content if present
    if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
        claude_content.push(json!({
            "type": "text",
            "text": text
        }));
    }
    
    // Add tool calls if present
    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        for call in tool_calls {
            claude_content.push(json!({
                "type": "tool_use",
                "id": call["id"],
                "name": call["function"]["name"],
                "input": serde_json::from_str::<Value>(
                    call["function"]["arguments"].as_str().unwrap_or("{}")
                ).unwrap_or(json!({}))
            }));
        }
    }
    
    // Extract usage
    let usage = response.get("usage");
    
    Ok(json!({
        "content": claude_content,
        "stop_reason": response["choices"][0]["finish_reason"].as_str().unwrap_or("end_turn"),
        "usage": {
            "input_tokens": usage.and_then(|u| u["prompt_tokens"].as_i64()).unwrap_or(0),
            "output_tokens": usage.and_then(|u| u["completion_tokens"].as_i64()).unwrap_or(0),
        }
    }))
}

/// Convert tool results to provider-specific format
pub fn tool_results_to_provider_messages(
    tool_results: Vec<Value>,
    provider: &str,
) -> Vec<Value> {
    match provider {
        "claude" => {
            // Claude uses tool_result format
            tool_results
                .into_iter()
                .map(|r| json!({
                    "type": "tool_result",
                    "tool_use_id": r["tool_use_id"],
                    "content": r["content"]
                }))
                .collect()
        }
        "gpt5" => {
            // GPT-5 uses function_call_output format
            tool_results
                .into_iter()
                .map(|r| json!({
                    "type": "function_call_output",
                    "call_id": r["tool_use_id"],
                    "output": r["content"].as_str().unwrap_or("")
                }))
                .collect()
        }
        "deepseek" => {
            // DeepSeek uses OpenAI tool message format
            tool_results
                .into_iter()
                .map(|r| json!({
                    "role": "tool",
                    "tool_call_id": r["tool_use_id"],
                    "content": r["content"]
                }))
                .collect()
        }
        _ => tool_results,
    }
}
