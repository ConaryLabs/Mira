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
    request_id: &str,
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
        request_id: request_id.to_owned(),
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

        let result = parse_chat_response(json, "test-123", 100).unwrap();
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

        let result = parse_chat_response(json, "test-456", 200).unwrap();
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

        let result = parse_chat_response(json, "test-789", 300).unwrap();
        assert_eq!(result.content, Some("The answer is 42.".to_string()));
        assert_eq!(
            result.reasoning_content,
            Some("Let me think about this...".to_string())
        );
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_chat_response("not json", "test", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_choices() {
        let json = r#"{"choices": [], "usage": null}"#;
        let result = parse_chat_response(json, "test", 0).unwrap();
        assert!(result.content.is_none());
        assert!(result.reasoning_content.is_none());
        assert!(result.tool_calls.is_none());
    }

    #[test]
    fn test_parse_usage_fields() {
        let json = r#"{
            "choices": [{"message": {"content": "ok"}}],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
                "prompt_cache_hit_tokens": 80,
                "prompt_cache_miss_tokens": 20
            }
        }"#;
        let result = parse_chat_response(json, "test", 0).unwrap();
        let usage = result.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
        assert_eq!(usage.prompt_cache_hit_tokens, Some(80));
        assert_eq!(usage.prompt_cache_miss_tokens, Some(20));
    }

    #[test]
    fn test_parse_multiple_tool_calls() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [
                        {"id": "call_1", "type": "function", "function": {"name": "search", "arguments": "{}"}},
                        {"id": "call_2", "type": "function", "function": {"name": "recall", "arguments": "{\"q\":\"x\"}"}}
                    ]
                }
            }],
            "usage": null
        }"#;
        let result = parse_chat_response(json, "test", 0).unwrap();
        let calls = result.tool_calls.unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].function.name, "search");
        assert_eq!(calls[1].id, "call_2");
        assert_eq!(calls[1].function.name, "recall");
    }

    #[test]
    fn test_parse_preserves_request_id_and_duration() {
        let json = r#"{"choices": [{"message": {"content": "hi"}}], "usage": null}"#;
        let result = parse_chat_response(json, "req-abc-123", 42).unwrap();
        assert_eq!(result.request_id, "req-abc-123");
        assert_eq!(result.duration_ms, 42);
    }

    #[test]
    fn test_parse_tool_call_fields_mapped_correctly() {
        let json = r#"{
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "tc_1",
                        "type": "function",
                        "function": {"name": "memory", "arguments": "{\"action\":\"recall\"}"}
                    }]
                }
            }],
            "usage": null
        }"#;
        let result = parse_chat_response(json, "test", 0).unwrap();
        let call = &result.tool_calls.unwrap()[0];
        assert_eq!(call.id, "tc_1");
        assert_eq!(call.call_type, "function");
        assert_eq!(call.function.name, "memory");
        assert_eq!(call.function.arguments, r#"{"action":"recall"}"#);
        // item_id and thought_signature should be None (not in response format)
        assert!(call.item_id.is_none());
        assert!(call.thought_signature.is_none());
    }
}
