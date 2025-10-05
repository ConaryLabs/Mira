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
        
        // FIXED: Use Value::String for content
        messages.push(ChatMessage { 
            role, 
            content: Value::String(content) 
        });
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
        }
    }
    
    Ok(json!({
        "content": claude_content,
        "stop_reason": "tool_use"
    }))
}
