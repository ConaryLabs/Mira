//! Request building for streaming API calls

use anyhow::Result;
use serde_json::{json, Value};
use crate::llm::client::OpenAIClient;

/// Build the request body for streaming responses
pub fn build_request_body(
    client: &OpenAIClient,
    user_text: &str,
    system_prompt: Option<&str>,
    structured_json: bool,
) -> Result<Value> {
    // Build input messages
    let input = vec![
        json!({
            "role": "system",
            "content": [{ "type": "input_text", "text": system_prompt.unwrap_or("") }]
        }),
        json!({
            "role": "user",
            "content": [{ "type": "input_text", "text": user_text }]
        }),
    ];

    // Base request body
    let mut body = json!({
        "model": client.model(),
        "input": input,
        "text": { 
            "verbosity": normalize_verbosity(client.verbosity()),
        },
        "reasoning": { 
            "effort": normalize_effort(client.reasoning_effort()) 
        },
        "max_output_tokens": client.max_output_tokens(),
        "stream": true,
    });

    // Add JSON schema for structured mode
    if structured_json {
        body["text"]["format"] = build_json_schema();
    }

    Ok(body)
}

/// Build JSON schema for structured responses
fn build_json_schema() -> Value {
    json!({
        "type": "json_schema",
        "name": "mira_response",
        "schema": {
            "type": "object",
            "properties": {
                "output": { "type": "string" },
                "mood": { "type": "string" },
                "salience": { "type": "integer", "minimum": 0, "maximum": 10 },
                "summary": { "type": "string" },
                "memory_type": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "intent": { "type": "string" },
                "monologue": { "type": ["string", "null"] },
                "reasoning_summary": { "type": ["string", "null"] }
            },
            "required": [
                "output", "mood", "salience", "summary", "memory_type",
                "tags", "intent"
            ],
            "additionalProperties": false
        }
    })
}

fn normalize_verbosity(v: &str) -> &'static str {
    match v {
        "concise" => "concise",
        "verbose" => "verbose",
        _ => "concise"
    }
}

fn normalize_effort(r: &str) -> &'static str {
    match r {
        "low" => "low",
        "medium" => "medium", 
        "high" => "high",
        _ => "medium"
    }
}
