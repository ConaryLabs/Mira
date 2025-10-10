// src/llm/provider/stream.rs
// Stream event types for GPT-5 Responses API SSE streaming

use serde_json::Value;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text delta - forward to client immediately
    TextDelta { delta: String },
    
    /// Reasoning delta - optional display
    ReasoningDelta { delta: String },
    
    /// Tool call started - pause stream and execute
    ToolCallStart {
        id: String,
        name: String,
    },
    
    /// Tool call arguments chunk
    ToolCallArgumentsDelta {
        id: String,
        delta: String,
    },
    
    /// Tool call complete - ready to execute
    ToolCallComplete {
        id: String,
        name: String,
        arguments: Value,
    },
    
    /// Stream completed successfully
    Done {
        response_id: String,
        input_tokens: i64,
        output_tokens: i64,
        reasoning_tokens: i64,
    },
    
    /// Stream failed with error
    Error {
        message: String,
    },
}

impl StreamEvent {
    /// Parse SSE event from GPT-5 Responses API
    pub fn from_sse_line(line: &str) -> Option<Self> {
        // SSE format: "event: <type>\ndata: <json>\n\n"
        // We'll receive lines like "data: {...}"
        
        if !line.starts_with("data: ") {
            return None;
        }
        
        let data = &line[6..]; // Skip "data: "
        
        // Handle special cases
        if data == "[DONE]" {
            // This is a completion marker, but we need response_id from final event
            return None;
        }
        
        // Parse JSON
        let json: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return None,
        };
        
        // Determine event type from the response structure
        // GPT-5 SSE format varies, so we need to check multiple patterns
        
        // Check for completion event
        if let Some(status) = json.get("status").and_then(|s| s.as_str()) {
            if status == "completed" {
                let response_id = json["id"].as_str().unwrap_or("").to_string();
                let usage = &json["usage"];
                return Some(StreamEvent::Done {
                    response_id,
                    input_tokens: usage["input_tokens"].as_i64().unwrap_or(0),
                    output_tokens: usage["output_tokens"].as_i64().unwrap_or(0),
                    reasoning_tokens: usage["output_tokens_details"]["reasoning_tokens"].as_i64().unwrap_or(0),
                });
            }
        }
        
        // Check for error
        if let Some(error) = json.get("error") {
            return Some(StreamEvent::Error {
                message: error["message"].as_str().unwrap_or("Unknown error").to_string(),
            });
        }
        
        // Check for output delta (text or reasoning)
        if let Some(output_array) = json.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                let item_type = item.get("type").and_then(|t| t.as_str());
                
                match item_type {
                    Some("message_delta") => {
                        // Text content delta
                        if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                            for content in content_array {
                                if content.get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                                    if let Some(delta) = content.get("text").and_then(|t| t.as_str()) {
                                        return Some(StreamEvent::TextDelta {
                                            delta: delta.to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Some("reasoning_delta") => {
                        if let Some(delta) = item.get("text").and_then(|t| t.as_str()) {
                            return Some(StreamEvent::ReasoningDelta {
                                delta: delta.to_string(),
                            });
                        }
                    }
                    Some("tool_call_delta") => {
                        // Tool call in progress
                        let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        
                        if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                            return Some(StreamEvent::ToolCallStart {
                                id: id.clone(),
                                name: name.to_string(),
                            });
                        }
                        
                        if let Some(delta) = item.get("arguments_delta").and_then(|a| a.as_str()) {
                            return Some(StreamEvent::ToolCallArgumentsDelta {
                                id,
                                delta: delta.to_string(),
                            });
                        }
                    }
                    Some("tool_call") => {
                        // Complete tool call
                        let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                        let arguments = item.get("arguments").cloned().unwrap_or(Value::Null);
                        
                        return Some(StreamEvent::ToolCallComplete {
                            id,
                            name,
                            arguments,
                        });
                    }
                    _ => {}
                }
            }
        }
        
        None
    }
}
