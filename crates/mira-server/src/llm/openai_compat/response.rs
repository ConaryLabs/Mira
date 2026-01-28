// crates/mira-server/src/llm/openai_compat/response.rs
// OpenAI-compatible chat response parsing

use crate::llm::{ChatResult, FunctionCall, ToolCall, Usage};
use anyhow::{Result, anyhow};
use serde::Deserialize;

/// Non-streaming chat response (OpenAI-compatible format)
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ResponseChoice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseChoice {
    pub message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
pub struct ResponseMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ResponseFunction,
}

#[derive(Debug, Deserialize)]
pub struct ResponseFunction {
    pub name: String,
    pub arguments: String,
}

/// Parse an OpenAI-compatible chat response into a ChatResult
pub fn parse_chat_response(
    response_body: &str,
    request_id: String,
    duration_ms: u64,
) -> Result<ChatResult> {
    let data: ChatResponse = serde_json::from_str(response_body)
        .map_err(|e| anyhow!("Failed to parse chat response: {}", e))?;

    // Extract response from first choice
    let choice = data.choices.into_iter().next();
    let (content, reasoning_content, tool_calls) = match choice {
        Some(c) => {
            let msg = c.message;
            let tc: Option<Vec<ToolCall>> = msg.tool_calls.map(|calls| {
                calls
                    .into_iter()
                    .map(|tc| ToolCall {
                        id: tc.id,
                        item_id: None,
                        call_type: tc.call_type,
                        function: FunctionCall {
                            name: tc.function.name,
                            arguments: tc.function.arguments,
                        },
                        thought_signature: None,
                    })
                    .collect()
            });
            (msg.content, msg.reasoning_content, tc)
        }
        None => (None, None, None),
    };

    Ok(ChatResult {
        request_id,
        content,
        reasoning_content,
        tool_calls,
        usage: data.usage,
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_response() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": "Hello, world!"
                }
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }"#;

        let result = parse_chat_response(json, "test-123".into(), 100).unwrap();
        assert_eq!(result.request_id, "test-123");
        assert_eq!(result.content, Some("Hello, world!".to_string()));
        assert!(result.tool_calls.is_none());
        assert_eq!(result.duration_ms, 100);
    }

    #[test]
    fn test_parse_response_with_tool_calls() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{\"query\": \"test\"}"
                        }
                    }]
                }
            }],
            "usage": null
        }"#;

        let result = parse_chat_response(json, "test-456".into(), 200).unwrap();
        assert!(result.content.is_none());
        let calls = result.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_123");
        assert_eq!(calls[0].function.name, "search");
    }

    #[test]
    fn test_parse_response_with_reasoning() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": "The answer is 42.",
                    "reasoning_content": "Let me think about this..."
                }
            }],
            "usage": null
        }"#;

        let result = parse_chat_response(json, "test-789".into(), 300).unwrap();
        assert_eq!(result.content, Some("The answer is 42.".to_string()));
        assert_eq!(
            result.reasoning_content,
            Some("Let me think about this...".to_string())
        );
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_chat_response("not json", "test".into(), 0);
        assert!(result.is_err());
    }
}
