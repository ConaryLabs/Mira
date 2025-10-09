// src/llm/provider/conversion.rs
// Format conversion helpers for translating between provider formats

use super::{Message, Response};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Convert Response to legacy format for backward compatibility  
pub fn to_legacy_format(response: &Response) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": response.content
        }],
        "usage": {
            "input_tokens": response.tokens.input,  // FIXED: Already i64, no unwrap_or
            "output_tokens": response.tokens.output,  // FIXED: Already i64, no unwrap_or
            "thinking_tokens": response.tokens.reasoning,
            "cached_tokens": response.tokens.cached
        },
        "model": response.model,
        "stop_reason": "end_turn"
    })
}

/// Extract messages from Claude-format request
pub fn extract_messages_from_claude_format(request: &Value) -> Result<Vec<Message>> {
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
        
        messages.push(Message { role, content });
    }
    
    Ok(messages)
}

/// Convert Message to Claude format
pub fn to_claude_message(message: &Message) -> Value {
    json!({
        "role": message.role,
        "content": message.content
    })
}

/// Convert Message to OpenAI/DeepSeek format
pub fn to_openai_message(message: &Message) -> Value {
    json!({
        "role": message.role,
        "content": message.content
    })
}

/// Convert provider Response to common format
pub fn from_provider_response(response: Response) -> Value {
    json!({
        "role": "assistant",
        "content": response.content,
    })
}

/// Convert GPT-5 tool response to Claude-compatible format
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
        }
    }
    
    Ok(json!({
        "content": claude_content,
        "stop_reason": "tool_use"
    }))
}
