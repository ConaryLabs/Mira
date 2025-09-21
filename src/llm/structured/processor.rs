// src/llm/structured/processor.rs
// FIXED: Using proper GPT-5 Responses API format

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tracing::debug;

use crate::CONFIG;
use super::types::*;

fn estimate_input_tokens(system_prompt: &str, context_messages: &[Value], user_message: &str) -> usize {
    let system_tokens = system_prompt.len() / 4;
    let context_tokens: usize = context_messages.iter()
        .map(|m| m.to_string().len() / 4)
        .sum();
    let user_tokens = user_message.len() / 4;
    
    system_tokens + context_tokens + user_tokens
}

pub fn build_structured_request(
    user_message: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
    // FIXED: Combine everything into a single input string (Responses API format)
    let mut full_input = system_prompt;
    
    // Add context messages as plain text
    for msg in context_messages {
        if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
            if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                full_input.push_str(&format!("\n\n{}: {}", role, content));
            } else if let Some(content_array) = msg.get("content").and_then(|c| c.as_array()) {
                // Handle nested content format from existing context
                for content_item in content_array {
                    if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                        full_input.push_str(&format!("\n\n{}: {}", role, text));
                    }
                }
            }
        }
    }
    
    // Add user message at the end
    full_input.push_str(&format!("\n\nUser: {}", user_message));
    
    // FIXED: Use Responses API format - single input string, not array
    Ok(json!({
        "model": CONFIG.gpt5_model,
        "input": full_input,  // Single string, not array
        "text": {
            "verbosity": CONFIG.verbosity,
            "format": {
                "type": "json_schema",
                "name": "mira_structured_response",
                "schema": get_response_schema(),  // FIXED: Direct schema, not nested
                "strict": true
            }
        },
        "reasoning": {
            "effort": CONFIG.reasoning_effort
        }
    }))
}

fn get_response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "output": {
                "type": "string",
                "description": "The main response content"
            },
            "analysis": {
                "type": "object",
                "properties": {
                    "salience": {
                        "type": "number",
                        "minimum": 0.0,
                        "maximum": 10.0,
                        "description": "Importance score from 0-10"
                    },
                    "topics": {
                        "type": "array",
                        "items": {"type": "string"},
                        "minItems": 1,
                        "description": "Key topics discussed"
                    },
                    "contains_code": {
                        "type": "boolean",
                        "description": "Whether response contains code"
                    },
                    "routed_to_heads": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["semantic", "code", "summary", "documents"]
                        },
                        "minItems": 1,
                        "description": "Memory heads for storage"
                    },
                    "language": {
                        "type": "string",
                        "default": "en",
                        "description": "Response language"
                    },
                    "mood": {
                        "type": ["string", "null"],
                        "description": "Optional mood indicator"
                    },
                    "intensity": {
                        "type": ["number", "null"],
                        "minimum": 0.0,
                        "maximum": 1.0,
                        "description": "Optional intensity score"
                    },
                    "intent": {
                        "type": ["string", "null"],
                        "description": "User intent classification"
                    },
                    "summary": {
                        "type": ["string", "null"],
                        "description": "Brief summary of response"
                    },
                    "relationship_impact": {
                        "type": ["string", "null"],
                        "description": "Impact on user relationship"
                    },
                    "programming_lang": {
                        "type": ["string", "null"],
                        "enum": ["rust", "typescript", "javascript", "python", "go", "java", null],
                        "description": "Programming language if contains_code is true"
                    }
                },
                "required": ["salience", "topics", "contains_code", "routed_to_heads", "language", "mood", "intensity", "intent", "summary", "relationship_impact", "programming_lang"],
                "additionalProperties": false
            },
            "reasoning": {
                "type": ["string", "null"],
                "description": "Optional reasoning trace"
            }
        },
        "required": ["output", "analysis", "reasoning"],
        "additionalProperties": false
    })
}

pub fn extract_metadata(raw_response: &Value, latency_ms: i64) -> Result<GPT5Metadata> {
    let usage = raw_response.get("usage");
    
    Ok(GPT5Metadata {
        response_id: raw_response.get("id").and_then(|v| v.as_str()).map(String::from),
        prompt_tokens: usage.and_then(|u| u.get("prompt_tokens")).and_then(|v| v.as_i64()),
        completion_tokens: usage.and_then(|u| u.get("completion_tokens")).and_then(|v| v.as_i64()),
        reasoning_tokens: usage.and_then(|u| u.get("reasoning_tokens")).and_then(|v| v.as_i64()),
        total_tokens: usage.and_then(|u| u.get("total_tokens")).and_then(|v| v.as_i64()),
        finish_reason: raw_response.get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(|v| v.as_str())
            .map(String::from),
        latency_ms,
        model_version: CONFIG.gpt5_model.clone(),
        temperature: 0.0,
        max_tokens: CONFIG.max_json_output_tokens as i64,
        reasoning_effort: CONFIG.reasoning_effort.clone(),
        verbosity: CONFIG.verbosity.clone(),
    })
}

pub fn extract_structured_content(raw_response: &Value) -> Result<StructuredGPT5Response> {
    // FIXED: GPT-5 Responses API returns array format
    // The JSON is in output[1].content[0].text as a string
    
    // Method 1: Handle GPT-5 Responses API array format
    if let Some(output_array) = raw_response.get("output").and_then(|v| v.as_array()) {
        // Look for the message with type "message"
        for item in output_array {
            if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                    for content_item in content_array {
                        if content_item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                            if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                                // Parse the JSON string
                                if let Ok(structured) = serde_json::from_str::<StructuredGPT5Response>(text) {
                                    return Ok(structured);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Method 2: Check if response has direct output field (fallback)
    if let Some(output) = raw_response.get("output") {
        // Try parsing output directly as JSON
        if let Ok(structured) = serde_json::from_value::<StructuredGPT5Response>(output.clone()) {
            return Ok(structured);
        }
        
        // Try parsing output as string containing JSON
        if let Some(output_str) = output.as_str() {
            if let Ok(structured) = serde_json::from_str::<StructuredGPT5Response>(output_str) {
                return Ok(structured);
            }
        }
    }
    
    // Method 3: Check if response has text field 
    if let Some(text_field) = raw_response.get("text") {
        if let Some(text_str) = text_field.as_str() {
            if let Ok(structured) = serde_json::from_str::<StructuredGPT5Response>(text_str) {
                return Ok(structured);
            }
        }
    }
    
    // Method 4: Check if response has output_text field
    if let Some(output_text) = raw_response.get("output_text").and_then(|v| v.as_str()) {
        if let Ok(structured) = serde_json::from_str::<StructuredGPT5Response>(output_text) {
            return Ok(structured);
        }
    }
    
    // Method 5: Check if the entire response is the JSON structure
    if let Ok(structured) = serde_json::from_value::<StructuredGPT5Response>(raw_response.clone()) {
        return Ok(structured);
    }
    
    // Debug: Let's see what's actually in the output field
    if let Some(output) = raw_response.get("output") {
        return Err(anyhow!(
            "Could not parse output field. Output type: {}, Output content: {}",
            if output.is_string() { "string" } else if output.is_object() { "object" } else if output.is_array() { "array" } else { "other" },
            serde_json::to_string_pretty(output).unwrap_or_else(|_| "unparseable".to_string())
        ));
    }
    
    Err(anyhow!(
        "Could not find structured content in response. Response keys: {:?}", 
        raw_response.as_object().map(|o| o.keys().collect::<Vec<_>>())
    ))
}
