// src/llm/structured/processor.rs

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
    let mut input = vec![];
    
    input.push(json!({
        "role": "system",
        "content": [{
            "type": "input_text",
            "text": system_prompt
        }]
    }));
    
    for msg in context_messages {
        input.push(msg);
    }
    
    input.push(json!({
        "role": "user",
        "content": [{
            "type": "input_text",
            "text": user_message
        }]
    }));
    
    Ok(json!({
        "model": CONFIG.gpt5_model,
        "input": input,
        "stream": false,
        "max_output_tokens": CONFIG.max_json_output_tokens,
        "text": {
            "verbosity": CONFIG.verbosity,
            "format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "mira_structured_response",
                    "strict": true,
                    "schema": get_response_schema()
                }
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
                        "type": "string",
                        "description": "Optional mood indicator"
                    },
                    "intensity": {
                        "type": "number",
                        "minimum": 0.0,
                        "maximum": 1.0,
                        "description": "Optional intensity score"
                    },
                    "intent": {
                        "type": "string",
                        "description": "User intent classification"
                    },
                    "summary": {
                        "type": "string",
                        "description": "Brief summary of response"
                    },
                    "relationship_impact": {
                        "type": "string",
                        "description": "Impact on user relationship"
                    },
                    "programming_lang": {
                        "type": "string",
                        "enum": ["rust", "typescript", "javascript", "python", "go", "java"],
                        "description": "Programming language if contains_code is true"
                    }
                },
                "required": ["salience", "topics", "contains_code", "routed_to_heads", "language"],
                "additionalProperties": false
            },
            "reasoning": {
                "type": "string",
                "description": "Optional reasoning trace"
            }
        },
        "required": ["output", "analysis"],
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
    let content = raw_response
        .get("output")
        .and_then(|output| output.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .ok_or_else(|| anyhow!("Could not find structured content in response"))?;
    
    let structured: StructuredGPT5Response = if content.is_string() {
        serde_json::from_str(content.as_str().unwrap())?
    } else {
        serde_json::from_value(content.clone())?
    };
    
    Ok(structured)
}
