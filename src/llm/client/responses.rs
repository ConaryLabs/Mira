// src/llm/client/responses.rs
// Phase 2: Extract Response Processing from client.rs
// Handles response format parsing, text extraction, and response validation

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, error};

/// Response output structure
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

/// Helper function to extract text content from various Responses API shapes.
/// This handles the complexity of different response formats from the OpenAI API.
pub fn extract_text_from_responses(response: &Value) -> Option<String> {
    // New primary path based on logs - /output/1/content/0/text
    if let Some(text) = response.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
        debug!("Extracted text using primary path: /output/1/content/0/text");
        return Some(text.to_string());
    }
    
    // 1) Newer shape: output.message.content[0].text.value
    if let Some(text) = response.pointer("/output/message/content/0/text/value").and_then(|t| t.as_str()) {
        debug!("Extracted text using: /output/message/content/0/text/value");
        return Some(text.to_string());
    }
    
    // 2) output.message.content[0].text
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
    
    // 3) message.content[0].text.value
    if let Some(text) = response.pointer("/message/content/0/text/value").and_then(|t| t.as_str()) {
        debug!("Extracted text using: /message/content/0/text/value");
        return Some(text.to_string());
    }
    
    // 4) message.content[0].text
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
    
    // 5) Fallback: choices[0].message.content (older API format)
    if let Some(text) = response.pointer("/choices/0/message/content").and_then(|t| t.as_str()) {
        debug!("Extracted text using fallback: choices[0].message.content");
        return Some(text.to_string());
    }
    
    // 6) Fallback: output as a raw string
    if let Some(text) = response.get("output").and_then(|o| o.as_str()) {
        debug!("Extracted text using: output as string");
        return Some(text.to_string());
    }
    
    // 7) Fallback for tool_calls
    if let Some(text) = response.pointer("/choices/0/message/tool_calls/0/function/arguments").and_then(|t| t.as_str()) {
        debug!("Extracted text using: tool_calls function arguments");
        return Some(text.to_string());
    }

    // 8) Try output array format
    if let Some(output_array) = response.get("output").and_then(|o| o.as_array()) {
        for item in output_array {
            if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            debug!("Extracted text using: output array format");
                            return Some(text.to_string());
                        }
                    }
                }
            }
        }
    }

    error!("Failed to extract text from response. Response structure: {}", 
           serde_json::to_string_pretty(response).unwrap_or_default());
    None
}

/// Extract tool calls from response
pub fn extract_tool_calls(response: &Value) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();

    // Try different paths for tool calls
    if let Some(calls) = response.pointer("/choices/0/message/tool_calls").and_then(|t| t.as_array()) {
        for call in calls {
            if let Ok(tool_call) = serde_json::from_value(call.clone()) {
                tool_calls.push(tool_call);
            }
        }
    }

    // Try output format for tool calls
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

/// Extract usage information from response
pub fn extract_usage_info(response: &Value) -> Option<UsageInfo> {
    if let Some(usage) = response.get("usage") {
        serde_json::from_value(usage.clone()).ok()
    } else {
        None
    }
}

/// Validate response structure
pub fn validate_response(response: &Value) -> Result<()> {
    // Check if response has expected structure
    if response.get("error").is_some() {
        if let Some(error_msg) = response.pointer("/error/message").and_then(|m| m.as_str()) {
            return Err(anyhow!("API Error: {}", error_msg));
        } else {
            return Err(anyhow!("API Error: {}", response.get("error").unwrap()));
        }
    }

    // Check for required fields
    if response.get("output").is_none() && response.get("choices").is_none() {
        return Err(anyhow!("Response missing both 'output' and 'choices' fields"));
    }

    Ok(())
}

/// Create structured JSON request body for the Responses API
pub fn create_request_body(
    user_text: &str,
    system_prompt: Option<&str>,
    model: &str,
    verbosity: &str,
    reasoning_effort: &str,
    max_output_tokens: usize,
    request_structured: bool,
) -> Value {
    let mut input = vec![serde_json::json!({
        "role": "user",
        "content": [{ "type": "input_text", "text": user_text }]
    })];

    if let Some(system) = system_prompt {
        input.insert(
            0,
            serde_json::json!({
                "role": "system",
                "content": [{ "type": "input_text", "text": system }]
            }),
        );
    }

    let mut request = serde_json::json!({
        "model": model,
        "input": input,
        "text": {
            "verbosity": normalize_verbosity(verbosity)
        },
        "reasoning": {
            "effort": normalize_reasoning_effort(reasoning_effort)
        },
        "max_output_tokens": max_output_tokens
    });

    if request_structured {
        // Add structured output format
        request["text"]["format"] = serde_json::json!({
            "type": "json_schema",
            "name": "mira_response",
            "schema": {
                "type": "object",
                "properties": {
                    "output": { "type":"string" },
                    "mood": { "type":"string" },
                    "salience": { "type":"integer", "minimum": 0, "maximum": 10 },
                    "summary": { "type":"string" },
                    "memory_type": { "type":"string" },
                    "tags": { "type":"array", "items": { "type":"string" } },
                    "intent": { "type":"string" },
                    "monologue": { "type":["string","null"] },
                    "reasoning_summary": { "type":["string","null"] }
                },
                "required": ["output","mood","salience","summary","memory_type","tags","intent","monologue","reasoning_summary"],
                "additionalProperties": false
            },
            "strict": true
        });
    }

    request
}

/// Normalize verbosity level to valid API values
pub fn normalize_verbosity(verbosity: &str) -> &'static str {
    match verbosity.to_ascii_lowercase().as_str() {
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }
}

/// Normalize reasoning effort to valid API values
pub fn normalize_reasoning_effort(effort: &str) -> &'static str {
    match effort.to_ascii_lowercase().as_str() {
        "minimal" => "minimal",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }
}

/// Tool call structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: Function,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub arguments: String,
}

/// Usage information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_text_primary_path() {
        let response = json!({
            "output": [
                {},
                {
                    "content": [
                        {
                            "text": "Hello, world!"
                        }
                    ]
                }
            ]
        });

        let text = extract_text_from_responses(&response);
        assert_eq!(text, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_extract_text_fallback_paths() {
        // Test choices format
        let response = json!({
            "choices": [
                {
                    "message": {
                        "content": "Fallback text"
                    }
                }
            ]
        });

        let text = extract_text_from_responses(&response);
        assert_eq!(text, Some("Fallback text".to_string()));
    }

    #[test]
    fn test_extract_text_output_string() {
        let response = json!({
            "output": "Direct string output"
        });

        let text = extract_text_from_responses(&response);
        assert_eq!(text, Some("Direct string output".to_string()));
    }

    #[test]
    fn test_normalize_verbosity() {
        assert_eq!(normalize_verbosity("LOW"), "low");
        assert_eq!(normalize_verbosity("Medium"), "medium");
        assert_eq!(normalize_verbosity("HIGH"), "high");
        assert_eq!(normalize_verbosity("invalid"), "medium");
    }

    #[test]
    fn test_normalize_reasoning_effort() {
        assert_eq!(normalize_reasoning_effort("minimal"), "minimal");
        assert_eq!(normalize_reasoning_effort("MEDIUM"), "medium");
        assert_eq!(normalize_reasoning_effort("high"), "high");
        assert_eq!(normalize_reasoning_effort("invalid"), "medium");
    }

    #[test]
    fn test_validate_response() {
        // Valid response
        let valid = json!({
            "output": "some content"
        });
        assert!(validate_response(&valid).is_ok());

        // Error response
        let error = json!({
            "error": {
                "message": "Invalid request"
            }
        });
        assert!(validate_response(&error).is_err());

        // Missing required fields
        let invalid = json!({
            "id": "test"
        });
        assert!(validate_response(&invalid).is_err());
    }

    #[test]
    fn test_create_request_body() {
        let body = create_request_body(
            "Hello",
            Some("You are helpful"),
            "gpt-5",
            "medium",
            "high",
            1000,
            false
        );

        assert_eq!(body["model"], "gpt-5");
        assert_eq!(body["max_output_tokens"], 1000);
        assert_eq!(body["text"]["verbosity"], "medium");
        assert_eq!(body["reasoning"]["effort"], "high");
        assert_eq!(body["input"].as_array().unwrap().len(), 2); // system + user
    }

    #[test]
    fn test_create_structured_request() {
        let body = create_request_body(
            "Hello",
            None,
            "gpt-5",
            "low",
            "minimal",
            500,
            true
        );

        assert!(body["text"]["format"].is_object());
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert!(body["text"]["format"]["schema"]["properties"].is_object());
    }
}
