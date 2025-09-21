// src/llm/structured/processor.rs
// Core processor for structured GPT-5 responses - no more streaming BS

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tracing::{debug, info};

// Core processor functions for structured GPT-5 responses

/// Estimate input tokens for monitoring purposes
pub fn estimate_input_tokens(system_prompt: &str, context_messages: &[Value], user_message: &str) -> usize {
    // Rough estimate: 1 token ≈ 4 characters
    let system_tokens = system_prompt.len() / 4;
    let context_tokens: usize = context_messages.iter()
        .map(|m| m.to_string().len() / 4)
        .sum();
    let user_tokens = user_message.len() / 4;
    
    system_tokens + context_tokens + user_tokens
}

/// Build the request with strict JSON schema enforcement
pub fn build_structured_request(
    user_message: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
    // Build input messages array
    let mut input = vec![];
    
    // System prompt first
    input.push(json!({
        "role": "system",
        "content": [{
            "type": "input_text",
            "text": system_prompt
        }]
    }));
    
    // Add context messages
    for msg in context_messages {
        input.push(msg);
    }
    
    // User message last
    input.push(json!({
        "role": "user",
        "content": [{
            "type": "input_text",
            "text": user_message
        }]
    }));
    
    // Build complete request with JSON schema enforcement
    Ok(json!({
        "model": CONFIG.gpt5_model,
        "input": input,
        "stream": false,  // CRITICAL: NO STREAMING!
        "max_output_tokens": CONFIG.max_json_output_tokens,  // High ceiling for code files
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

/// Get the JSON schema that enforces all database requirements
pub fn get_response_schema() -> Value {
    json!({
        "type": "object",
        "required": ["output", "analysis"],
        "properties": {
            "output": {
                "type": "string",
                "minLength": 1,
                "description": "The response to the user"
            },
            "analysis": {
                "type": "object",
                "required": ["salience", "topics", "contains_code", "routed_to_heads"],
                "properties": {
                    "salience": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 10,
                        "description": "Importance score 0-10"
                    },
                    "topics": {
                        "type": "array",
                        "minItems": 1,
                        "items": {"type": "string"},
                        "description": "Key topics discussed"
                    },
                    "contains_code": {
                        "type": "boolean",
                        "description": "Whether response contains code"
                    },
                    "routed_to_heads": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "string",
                            "enum": ["semantic", "code", "summary", "documents"]
                        },
                        "description": "Memory heads to route to"
                    },
                    "language": {
                        "type": "string",
                        "default": "en",
                        "description": "Language code"
                    },
                    "mood": {
                        "type": ["string", "null"],
                        "description": "Emotional tone"
                    },
                    "intensity": {
                        "type": ["number", "null"],
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Emotional intensity 0-1"
                    },
                    "intent": {
                        "type": ["string", "null"],
                        "description": "User intent"
                    },
                    "summary": {
                        "type": ["string", "null"],
                        "description": "Brief summary"
                    },
                    "relationship_impact": {
                        "type": ["string", "null"],
                        "description": "Impact on relationship"
                    },
                    "programming_lang": {
                        "type": ["string", "null"],
                        "enum": ["rust", "typescript", "javascript", "python", "go", "java", null],
                        "description": "Programming language if contains_code=true"
                    }
                },
                "additionalProperties": false
            },
            "reasoning": {
                "type": ["string", "null"],
                "description": "Optional reasoning for debugging"
            }
        },
        "additionalProperties": false
    })
}

/// Extract metadata from raw GPT-5 response
pub fn extract_metadata(raw_response: &Value, latency_ms: i32) -> Result<GPT5Metadata> {
    // Extract usage statistics
    let usage = raw_response.get("usage");
    let prompt_tokens = usage.and_then(|u| u.get("prompt_tokens")).and_then(|t| t.as_i64()).map(|t| t as i32);
    let completion_tokens = usage.and_then(|u| u.get("completion_tokens")).and_then(|t| t.as_i64()).map(|t| t as i32);
    let reasoning_tokens = usage.and_then(|u| u.get("reasoning_tokens")).and_then(|t| t.as_i64()).map(|t| t as i32);
    let total_tokens = usage.and_then(|u| u.get("total_tokens")).and_then(|t| t.as_i64()).map(|t| t as i32);
    
    // Extract response metadata
    let response_id = raw_response.get("id").and_then(|id| id.as_str()).map(|s| s.to_string());
    let finish_reason = raw_response.get("finish_reason").and_then(|f| f.as_str()).map(|s| s.to_string());
    
    Ok(GPT5Metadata {
        response_id,
        prompt_tokens,
        completion_tokens,
        reasoning_tokens,
        total_tokens,
        finish_reason,
        latency_ms,
        model_version: CONFIG.gpt5_model.clone(),
        temperature: 0.7, // TODO: Get from CONFIG
        max_tokens: CONFIG.max_json_output_tokens as i32,
        reasoning_effort: CONFIG.reasoning_effort.clone(),
        verbosity: CONFIG.verbosity.clone(),
    })
}

/// Extract structured content from raw response
pub fn extract_structured_content(raw_response: &Value) -> Result<StructuredGPT5Response> {
    // Try different extraction paths for GPT-5 Responses API
    let content = raw_response
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|first| first.get("text"))
        .or_else(|| raw_response.get("text"))
        .or_else(|| raw_response.get("message").and_then(|m| m.get("content")))
        .ok_or_else(|| anyhow!("Could not find structured content in response"))?;
    
    // Parse the JSON content
    let structured: StructuredGPT5Response = if content.is_string() {
        serde_json::from_str(content.as_str().unwrap())?
    } else {
        serde_json::from_value(content.clone())?
    };
    
    Ok(structured)
}
