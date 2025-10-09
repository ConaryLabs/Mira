// src/llm/provider/conversion.rs
// Format conversion helpers for translating between provider formats

use super::{Message, Response};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Extract messages from unified format request
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
        
        // FIXED: Use Value::String for content
        messages.push(Message { 
            role, 
            content: content 
        });
    }
    
    Ok(messages)
}

/// Convert Response to unified format Value for backward compatibility
pub fn provider_response_to_claude_format(response: Response) -> Value {
    let mut claude_response = json!({
        "content": [{
            "type": "text",
            "text": response.content
        }],
        "stop_reason": Some("end_turn".to_string()).unwrap_or_else(|| "end_turn".to_string()),
        "usage": {
            "input_tokens": response.tokens.input.unwrap_or(0),
            "output_tokens": response.tokens.output.unwrap_or(0),
        }
    });
    
    // Add thinking if present
    
    claude_response
}

/// Convert GPT-5 tool response to unified format
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
