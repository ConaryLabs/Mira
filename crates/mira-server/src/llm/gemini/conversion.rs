// crates/mira-server/src/llm/gemini/conversion.rs
// Message and tool conversion between Mira and Gemini formats

use crate::llm::gemini::types::{
    GeminiContent, GeminiFunctionCall, GeminiFunctionDeclaration, GeminiFunctionResponse,
    GeminiFunctionsTool, GeminiPart, GeminiTool, GoogleSearchConfig, GoogleSearchTool,
};
use crate::llm::{Message, Tool};
use serde_json::Value;
use std::collections::HashMap;

/// Convert Mira Message to Gemini Content
/// Returns (content, is_system) - system messages are handled separately
pub fn convert_message(
    msg: &Message,
    tool_id_map: Option<&HashMap<String, String>>,
) -> Option<(GeminiContent, bool)> {
    match msg.role.as_str() {
        "system" => {
            // System messages go to system_instruction
            let parts = vec![GeminiPart::Text {
                text: msg.content.clone().unwrap_or_default(),
                thought: false,
            }];
            Some((
                GeminiContent {
                    role: "user".into(), // system_instruction uses user role
                    parts,
                },
                true,
            ))
        }
        "user" => {
            let parts = vec![GeminiPart::Text {
                text: msg.content.clone().unwrap_or_default(),
                thought: false,
            }];
            Some((
                GeminiContent {
                    role: "user".into(),
                    parts,
                },
                false,
            ))
        }
        "assistant" => {
            let mut parts = Vec::new();

            // Add text content if present
            if let Some(ref content) = msg.content
                && !content.is_empty()
            {
                parts.push(GeminiPart::Text {
                    text: content.clone(),
                    thought: false,
                });
            }

            // Add function calls if present (include thought signatures for Gemini 3)
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    let args: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(Value::Object(Default::default()));
                    parts.push(GeminiPart::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: tc.function.name.clone(),
                            args,
                        },
                        thought_signature: tc.thought_signature.clone(),
                    });
                }
            }

            if parts.is_empty() {
                parts.push(GeminiPart::Text {
                    text: String::new(),
                    thought: false,
                });
            }

            Some((
                GeminiContent {
                    role: "model".into(),
                    parts,
                },
                false,
            ))
        }
        "tool" => {
            // Tool responses become function_response parts
            // We need to find the function name from context
            let tool_call_id = msg.tool_call_id.clone().unwrap_or_default();
            let function_name = tool_id_map
                .and_then(|m| m.get(&tool_call_id))
                .cloned()
                .unwrap_or_else(|| "unknown".into());

            // Gemini requires function_response.response to be a JSON object (Struct),
            // not a string or other primitive. Always ensure we return an object.
            let content_str = msg.content.as_deref().unwrap_or("");
            let response: Value = match serde_json::from_str::<Value>(content_str) {
                Ok(Value::Object(obj)) => Value::Object(obj),
                Ok(other) => serde_json::json!({ "result": other }),
                Err(_) => serde_json::json!({ "result": content_str }),
            };

            let parts = vec![GeminiPart::FunctionResponse {
                function_response: GeminiFunctionResponse {
                    name: function_name,
                    response,
                },
            }];

            Some((
                GeminiContent {
                    role: "user".into(),
                    parts,
                },
                false,
            ))
        }
        _ => None,
    }
}

/// Convert Mira Tools to Gemini function declarations tool
pub fn convert_tools(tools: &[Tool]) -> GeminiTool {
    let declarations: Vec<GeminiFunctionDeclaration> = tools
        .iter()
        .map(|t| GeminiFunctionDeclaration {
            name: t.function.name.clone(),
            description: t.function.description.clone(),
            parameters: t.function.parameters.clone(),
        })
        .collect();

    GeminiTool::Functions(GeminiFunctionsTool {
        function_declarations: declarations,
    })
}

/// Create Google Search tool
pub fn google_search_tool() -> GeminiTool {
    GeminiTool::GoogleSearch(GoogleSearchTool {
        google_search: GoogleSearchConfig {},
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::FunctionDef;

    // ============================================================================
    // convert_tools tests
    // ============================================================================

    #[test]
    fn test_convert_tools_single() {
        let tools = vec![Tool {
            tool_type: "function".to_string(),
            function: FunctionDef {
                name: "search".to_string(),
                description: "Search for things".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
        }];
        let result = convert_tools(&tools);
        match result {
            GeminiTool::Functions(funcs) => {
                assert_eq!(funcs.function_declarations.len(), 1);
                assert_eq!(funcs.function_declarations[0].name, "search");
            }
            _ => panic!("Expected Functions tool"),
        }
    }

    #[test]
    fn test_convert_tools_multiple() {
        let tools = vec![
            Tool {
                tool_type: "function".to_string(),
                function: FunctionDef {
                    name: "search".to_string(),
                    description: "Search".to_string(),
                    parameters: serde_json::json!({}),
                },
            },
            Tool {
                tool_type: "function".to_string(),
                function: FunctionDef {
                    name: "read".to_string(),
                    description: "Read".to_string(),
                    parameters: serde_json::json!({}),
                },
            },
        ];
        let result = convert_tools(&tools);
        match result {
            GeminiTool::Functions(funcs) => {
                assert_eq!(funcs.function_declarations.len(), 2);
            }
            _ => panic!("Expected Functions tool"),
        }
    }

    // ============================================================================
    // google_search_tool tests
    // ============================================================================

    #[test]
    fn test_google_search_tool_creation() {
        let tool = google_search_tool();
        match tool {
            GeminiTool::GoogleSearch(_) => {} // Expected
            _ => panic!("Expected GoogleSearch tool"),
        }
    }
}
