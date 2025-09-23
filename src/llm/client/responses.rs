// src/llm/client/responses.rs

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct ResponseOutput {
    pub content: String,
    pub raw: Option<Value>,
}

impl ResponseOutput {
    pub fn new(content: String) -> Self {
        Self {
            content,
            raw: None,
        }
    }

    pub fn with_raw(content: String, raw: Value) -> Self {
        Self {
            content,
            raw: Some(raw),
        }
    }
}

pub fn extract_text_from_responses(response: &Value) -> Option<String> {
    // PRIMARY PATH: GPT-5 September 2025 format
    // output[1].content[0].text (where output[0] is reasoning, output[1] is message)
    if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
        // Look for the message entry (usually output[1])
        for item in output_array {
            if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                    if let Some(first_content) = content_array.first() {
                        if let Some(text) = first_content.get("text").and_then(|t| t.as_str()) {
                            debug!("Extracted text using: output[message].content[0].text");
                            return Some(text.to_string());
                        }
                    }
                }
            }
        }
    }
    
    // FALLBACK: Try the old paths for backwards compatibility
    
    // Try /output/1/content/0/text directly (based on logs)
    if let Some(text) = response.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
        debug!("Extracted text using: /output/1/content/0/text");
        return Some(text.to_string());
    }
    
    // output.message.content[0].text.value
    if let Some(text) = response.pointer("/output/message/content/0/text/value").and_then(|t| t.as_str()) {
        debug!("Extracted text using: /output/message/content/0/text/value");
        return Some(text.to_string());
    }
    
    // output.message.content[0].text
    if let Some(text) = response
        .get("output").and_then(|o| o.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.get(0))
        .and_then(|part| part.get("text"))
        .and_then(|t| t.as_str())
    {
        debug!("Extracted text using: output.message.content[0].text");
        return Some(text.to_string());
    }
    
    // message.content[0].text.value
    if let Some(text) = response.pointer("/message/content/0/text/value").and_then(|t| t.as_str()) {
        debug!("Extracted text using: /message/content/0/text/value");
        return Some(text.to_string());
    }
    
    // message.content[0].text
    if let Some(text) = response
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.get(0))
        .and_then(|part| part.get("text"))
        .and_then(|t| t.as_str())
    {
        debug!("Extracted text using: message.content[0].text");
        return Some(text.to_string());
    }
    
    // Fallback: choices[0].message.content (older API format)
    if let Some(text) = response.pointer("/choices/0/message/content").and_then(|t| t.as_str()) {
        debug!("Extracted text using fallback: choices[0].message.content");
        return Some(text.to_string());
    }
    
    // Try output[0].text directly
    if let Some(text) = response.pointer("/output/0/text").and_then(|t| t.as_str()) {
        debug!("Extracted text using: /output/0/text");
        return Some(text.to_string());
    }
    
    // Fallback: output as a raw string
    if let Some(text) = response.get("output").and_then(|o| o.as_str()) {
        debug!("Extracted text using: output as string");
        return Some(text.to_string());
    }
    
    // Convenience field: output_text
    if let Some(text) = response.get("output_text").and_then(|t| t.as_str()) {
        debug!("Extracted text using: output_text field");
        return Some(text.to_string());
    }
    
    // Try iterating through output array (legacy path)
    if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
        for (i, item) in output_array.iter().enumerate() {
            // Try direct text field
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                debug!("Extracted text using: output[{}].text", i);
                return Some(text.to_string());
            }
            
            if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                for content_item in content_array {
                    if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                        debug!("Extracted text using: output[{}].content[].text", i);
                        return Some(text.to_string());
                    }
                    if content_item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                        if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                            debug!("Extracted text using: output array format");
                            return Some(text.to_string());
                        }
                    }
                }
            }
        }
    }

    error!("Failed to extract text from response. Tried all known extraction paths.");
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i32>,
}

pub fn extract_tool_calls(response: &Value) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();

    if let Some(calls) = response.pointer("/choices/0/message/tool_calls").and_then(|t| t.as_array()) {
        for call in calls {
            if let Ok(tool_call) = serde_json::from_value(call.clone()) {
                tool_calls.push(tool_call);
            }
        }
    }

    if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
        for item in output_array {
            if item.get("type").and_then(|t| t.as_str()) == Some("tool_call") {
                if let Ok(tool_call) = serde_json::from_value(item.clone()) {
                    tool_calls.push(tool_call);
                }
            }
        }
    }

    tool_calls
}

pub fn extract_usage_info(response: &Value) -> Option<UsageInfo> {
    if let Some(usage) = response.get("usage") {
        serde_json::from_value(usage.clone()).ok()
    } else {
        None
    }
}

pub fn validate_response(response: &Value) -> Result<()> {
    if response.get("error").is_some() {
        if let Some(error_msg) = response.pointer("/error/message").and_then(|m| m.as_str()) {
            return Err(anyhow!("API Error: {}", error_msg));
        } else {
            return Err(anyhow!("API Error: {}", response.get("error").unwrap()));
        }
    }

    if response.get("output").is_none() && response.get("choices").is_none() {
        return Err(anyhow!("Response missing both 'output' and 'choices' fields"));
    }

    Ok(())
}

fn normalize_verbosity(verbosity: &str) -> &str {
    match verbosity.to_lowercase().as_str() {
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }
}

fn normalize_reasoning_effort(effort: &str) -> &str {
    match effort.to_lowercase().as_str() {
        "minimal" => "minimal",
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }
}

pub fn create_request_body(
    user_text: &str,
    system_prompt: Option<&str>,
    model: &str,
    verbosity: &str,
    reasoning_effort: &str,
    max_output_tokens: usize,
    request_structured: bool,
) -> Value {
    let mut input = vec![];
    
    if let Some(system) = system_prompt {
        input.push(serde_json::json!({
            "role": "system",
            "content": [{ "type": "input_text", "text": system }]
        }));
    }
    
    input.push(serde_json::json!({
        "role": "user",
        "content": [{ "type": "input_text", "text": user_text }]
    }));

    let mut request = serde_json::json!({
        "model": model,
        "input": input,
        "max_output_tokens": max_output_tokens,
    });

    let mut text_config = serde_json::json!({
        "verbosity": normalize_verbosity(verbosity)
    });
    
    if request_structured {
        text_config["format"] = serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": "structured_output",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "output": { "type": "string" },
                        "metadata": {
                            "type": "object",
                            "properties": {
                                "mood": { "type": "string" },
                                "salience": { "type": "number", "minimum": 0, "maximum": 1 },
                                "summary": { "type": "string" },
                                "memory_type": { "type": "string" },
                                "tags": { "type": "array", "items": { "type": "string" } },
                                "intent": { "type": "string" }
                            },
                            "required": ["mood", "salience", "summary", "memory_type", "tags", "intent"]
                        },
                        "reasoning": {
                            "type": "object",
                            "properties": {
                                "monologue": { "type": "string" },
                                "summary": { "type": "string" }
                            },
                            "required": ["monologue", "summary"]
                        }
                    },
                    "required": ["output", "metadata", "reasoning"]
                }
            }
        });
    } else {
        text_config["format"] = serde_json::json!({
            "type": "text"
        });
    }
    
    request["text"] = text_config;

    request["reasoning"] = serde_json::json!({
        "effort": normalize_reasoning_effort(reasoning_effort)
    });

    request
}
