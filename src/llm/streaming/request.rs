// src/llm/streaming/request.rs

use anyhow::Result;
use serde_json::{json, Value};
use crate::llm::client::OpenAIClient;

pub fn build_request_body(
    client: &OpenAIClient,
    user_text: &str,
    system_prompt: Option<&str>,
    structured_json: bool,
) -> Result<Value> {
    let mut input = vec![];
    
    if let Some(prompt) = system_prompt {
        if !prompt.is_empty() {
            input.push(json!({
                "role": "system",
                "content": [{ "type": "input_text", "text": prompt }]
            }));
        }
    }
    
    input.push(json!({
        "role": "user",
        "content": [{ "type": "input_text", "text": user_text }]
    }));
    
    let mut body = json!({
        "model": client.model(),
        "input": input,
        "stream": true,
        "max_output_tokens": client.max_output_tokens(),
        "text": { 
            "verbosity": normalize_verbosity(client.verbosity()),
        },
        "reasoning": { 
            "effort": normalize_effort(client.reasoning_effort()) 
        }
    });
    
    if structured_json {
        body["text"]["format"] = build_json_schema();
    }
    
    Ok(body)
}

fn build_json_schema() -> Value {
    json!({
        "type": "json_schema",
        "json_schema": {
            "name": "mira_response",
            "strict": true,
            "schema": {
                "type": "object",
                "properties": {
                    "output": { 
                        "type": "string",
                        "description": "The main response content"
                    },
                    "mood": { 
                        "type": "string",
                        "description": "The emotional tone of the response"
                    },
                    "salience": { 
                        "type": "integer", 
                        "minimum": 0, 
                        "maximum": 10,
                        "description": "Importance score for memory retention"
                    },
                    "summary": { 
                        "type": "string",
                        "description": "Brief summary of the response"
                    },
                    "memory_type": { 
                        "type": "string",
                        "description": "Category of memory entry"
                    },
                    "tags": { 
                        "type": "array", 
                        "items": { "type": "string" },
                        "description": "Keywords for retrieval"
                    },
                    "intent": { 
                        "type": "string",
                        "description": "Detected user intent"
                    },
                    "monologue": { 
                        "type": ["string", "null"],
                        "description": "Internal reasoning notes"
                    },
                    "reasoning_summary": { 
                        "type": ["string", "null"],
                        "description": "Summary of reasoning process"
                    }
                },
                "required": [
                    "output", 
                    "mood", 
                    "salience", 
                    "summary", 
                    "memory_type",
                    "tags", 
                    "intent"
                ],
                "additionalProperties": false
            }
        }
    })
}

fn normalize_verbosity(v: &str) -> &'static str {
    match v.to_lowercase().as_str() {
        "low" | "concise" => "low",
        "medium" | "balanced" => "medium",
        "high" | "verbose" => "high",
        _ => "medium"
    }
}

fn normalize_effort(r: &str) -> &'static str {
    match r.to_lowercase().as_str() {
        "minimal" => "minimal",
        "low" => "low",
        "medium" => "medium", 
        "high" => "high",
        _ => "medium"
    }
}
