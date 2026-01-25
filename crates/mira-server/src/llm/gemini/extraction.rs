// crates/mira-server/src/llm/gemini/extraction.rs
// Response extraction helpers for Gemini API responses

use crate::llm::deepseek::{FunctionCall, ToolCall};
use crate::llm::gemini::types::{GeminiContent, GeminiPart};
#[cfg(test)]
use crate::llm::gemini::types::GeminiFunctionCall;

/// Extract tool calls from Gemini response
pub fn extract_tool_calls(content: &GeminiContent) -> Option<Vec<ToolCall>> {
    let mut tool_calls = Vec::new();

    for (idx, part) in content.parts.iter().enumerate() {
        if let GeminiPart::FunctionCall { function_call, thought_signature } = part {
            tool_calls.push(ToolCall {
                id: format!("call_{}", idx),
                item_id: None,
                call_type: "function".into(),
                function: FunctionCall {
                    name: function_call.name.clone(),
                    arguments: serde_json::to_string(&function_call.args).unwrap_or_default(),
                },
                thought_signature: thought_signature.clone(),
            });
        }
    }

    if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    }
}

/// Extract text content from Gemini response (non-thought parts only)
pub fn extract_content(content: &GeminiContent) -> Option<String> {
    let text_parts: Vec<&str> = content
        .parts
        .iter()
        .filter_map(|part| {
            if let GeminiPart::Text { text, thought } = part {
                // Only include non-thought text
                if !thought {
                    Some(text.as_str())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    }
}

/// Extract thought summaries (reasoning) from Gemini response
pub fn extract_thoughts(content: &GeminiContent) -> Option<String> {
    let thought_parts: Vec<&str> = content
        .parts
        .iter()
        .filter_map(|part| {
            if let GeminiPart::Text { text, thought } = part {
                // Only include thought parts
                if *thought {
                    Some(text.as_str())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    if thought_parts.is_empty() {
        None
    } else {
        Some(thought_parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // extract_content tests
    // ============================================================================

    #[test]
    fn test_extract_content_single_text() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Hello world".to_string(),
                thought: false,
            }],
        };
        assert_eq!(
            extract_content(&content),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn test_extract_content_multiple_texts() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "Hello ".to_string(),
                    thought: false,
                },
                GeminiPart::Text {
                    text: "world".to_string(),
                    thought: false,
                },
            ],
        };
        assert_eq!(
            extract_content(&content),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn test_extract_content_skips_thoughts() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "I'm thinking...".to_string(),
                    thought: true,
                },
                GeminiPart::Text {
                    text: "Here's the answer".to_string(),
                    thought: false,
                },
            ],
        };
        assert_eq!(
            extract_content(&content),
            Some("Here's the answer".to_string())
        );
    }

    #[test]
    fn test_extract_content_only_thoughts_returns_none() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Thinking...".to_string(),
                thought: true,
            }],
        };
        assert_eq!(extract_content(&content), None);
    }

    #[test]
    fn test_extract_content_empty_parts() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![],
        };
        assert_eq!(extract_content(&content), None);
    }

    #[test]
    fn test_extract_content_skips_function_calls() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "Let me search".to_string(),
                    thought: false,
                },
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "search".to_string(),
                        args: serde_json::json!({}),
                    },
                    thought_signature: None,
                },
            ],
        };
        assert_eq!(
            extract_content(&content),
            Some("Let me search".to_string())
        );
    }

    // ============================================================================
    // extract_thoughts tests
    // ============================================================================

    #[test]
    fn test_extract_thoughts_single() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Analyzing the problem...".to_string(),
                thought: true,
            }],
        };
        assert_eq!(
            extract_thoughts(&content),
            Some("Analyzing the problem...".to_string())
        );
    }

    #[test]
    fn test_extract_thoughts_multiple() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "First thought".to_string(),
                    thought: true,
                },
                GeminiPart::Text {
                    text: "Second thought".to_string(),
                    thought: true,
                },
            ],
        };
        let thoughts = extract_thoughts(&content).unwrap();
        assert!(thoughts.contains("First thought"));
        assert!(thoughts.contains("Second thought"));
    }

    #[test]
    fn test_extract_thoughts_skips_non_thoughts() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "Thinking...".to_string(),
                    thought: true,
                },
                GeminiPart::Text {
                    text: "Final answer".to_string(),
                    thought: false,
                },
            ],
        };
        assert_eq!(
            extract_thoughts(&content),
            Some("Thinking...".to_string())
        );
    }

    #[test]
    fn test_extract_thoughts_no_thoughts_returns_none() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Just an answer".to_string(),
                thought: false,
            }],
        };
        assert_eq!(extract_thoughts(&content), None);
    }

    // ============================================================================
    // extract_tool_calls tests
    // ============================================================================

    #[test]
    fn test_extract_tool_calls_single() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::FunctionCall {
                function_call: GeminiFunctionCall {
                    name: "search".to_string(),
                    args: serde_json::json!({"query": "test"}),
                },
                thought_signature: None,
            }],
        };
        let calls = extract_tool_calls(&content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "search");
    }

    #[test]
    fn test_extract_tool_calls_multiple() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "search".to_string(),
                        args: serde_json::json!({"q": "1"}),
                    },
                    thought_signature: None,
                },
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "read".to_string(),
                        args: serde_json::json!({"path": "/tmp"}),
                    },
                    thought_signature: Some("sig123".to_string()),
                },
            ],
        };
        let calls = extract_tool_calls(&content).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].function.name, "search");
        assert_eq!(calls[1].function.name, "read");
        assert_eq!(calls[1].thought_signature, Some("sig123".to_string()));
    }

    #[test]
    fn test_extract_tool_calls_none_when_no_calls() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Just text".to_string(),
                thought: false,
            }],
        };
        assert_eq!(extract_tool_calls(&content), None);
    }

    #[test]
    fn test_extract_tool_calls_generates_ids() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "func1".to_string(),
                        args: serde_json::json!({}),
                    },
                    thought_signature: None,
                },
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "func2".to_string(),
                        args: serde_json::json!({}),
                    },
                    thought_signature: None,
                },
            ],
        };
        let calls = extract_tool_calls(&content).unwrap();
        assert_eq!(calls[0].id, "call_0");
        assert_eq!(calls[1].id, "call_1");
    }
}
