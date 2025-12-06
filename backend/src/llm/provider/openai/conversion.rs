// src/llm/provider/openai/conversion.rs
// Convert between internal message format and OpenAI API format

use serde_json::Value;

use super::types::{
    ChatMessage, FunctionCallMessage, FunctionDefinition, Tool, ToolCallMessage,
};
use crate::llm::provider::Message;

/// Convert internal messages to OpenAI chat messages
pub fn messages_to_openai(messages: &[Message], system: &str) -> Vec<ChatMessage> {
    let mut openai_messages = Vec::with_capacity(messages.len() + 1);

    // Add system message first
    if !system.is_empty() {
        openai_messages.push(ChatMessage {
            role: "system".to_string(),
            content: Some(system.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    for msg in messages {
        match msg.role.as_str() {
            "user" => {
                openai_messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(msg.content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
            }
            "assistant" => {
                // Check if this assistant message has tool calls
                if let Some(ref tool_calls) = msg.tool_calls {
                    let openai_tool_calls: Vec<ToolCallMessage> = tool_calls
                        .iter()
                        .map(|tc| ToolCallMessage {
                            id: tc.id.clone(),
                            call_type: "function".to_string(),
                            function: FunctionCallMessage {
                                name: tc.name.clone(),
                                arguments: tc.arguments.to_string(),
                            },
                        })
                        .collect();

                    openai_messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: if msg.content.is_empty() {
                            None
                        } else {
                            Some(msg.content.clone())
                        },
                        tool_calls: Some(openai_tool_calls),
                        tool_call_id: None,
                        name: None,
                    });
                } else {
                    openai_messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: Some(msg.content.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
            }
            "tool" => {
                // Tool result message
                openai_messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(msg.content.clone()),
                    tool_calls: None,
                    tool_call_id: msg.tool_call_id.clone(),
                    name: msg.tool_name.clone(),
                });
            }
            "system" => {
                // Skip system messages (already handled at start)
            }
            _ => {
                // Unknown role - treat as user
                openai_messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(msg.content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
            }
        }
    }

    openai_messages
}

/// Convert Gemini-style tool definitions to OpenAI format
///
/// Gemini format:
/// ```json
/// {
///   "functionDeclarations": [{
///     "name": "tool_name",
///     "description": "...",
///     "parameters": { ... }
///   }]
/// }
/// ```
///
/// OpenAI format:
/// ```json
/// [{
///   "type": "function",
///   "function": {
///     "name": "tool_name",
///     "description": "...",
///     "parameters": { ... }
///   }
/// }]
/// ```
pub fn tools_to_openai(gemini_tools: &[Value]) -> Vec<Tool> {
    let mut openai_tools = Vec::new();

    for tool_def in gemini_tools {
        // Handle Gemini's functionDeclarations format
        if let Some(declarations) = tool_def.get("functionDeclarations") {
            if let Some(arr) = declarations.as_array() {
                for func in arr {
                    if let (Some(name), Some(description)) =
                        (func.get("name"), func.get("description"))
                    {
                        let name = name.as_str().unwrap_or("unknown").to_string();
                        let description = description.as_str().unwrap_or("").to_string();
                        let parameters = func
                            .get("parameters")
                            .cloned()
                            .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));

                        openai_tools.push(Tool {
                            tool_type: "function".to_string(),
                            function: FunctionDefinition {
                                name,
                                description,
                                parameters,
                            },
                        });
                    }
                }
            }
        }
        // Also handle if tools are already in OpenAI format (just in case)
        else if let Some(func) = tool_def.get("function") {
            if let (Some(name), Some(description)) = (func.get("name"), func.get("description")) {
                let name = name.as_str().unwrap_or("unknown").to_string();
                let description = description.as_str().unwrap_or("").to_string();
                let parameters = func
                    .get("parameters")
                    .cloned()
                    .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));

                openai_tools.push(Tool {
                    tool_type: "function".to_string(),
                    function: FunctionDefinition {
                        name,
                        description,
                        parameters,
                    },
                });
            }
        }
    }

    openai_tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::ToolCallInfo;

    #[test]
    fn test_basic_message_conversion() {
        let messages = vec![
            Message::user("Hello".to_string()),
            Message::assistant("Hi there!".to_string()),
        ];

        let openai_msgs = messages_to_openai(&messages, "You are helpful.");

        assert_eq!(openai_msgs.len(), 3); // system + 2 messages
        assert_eq!(openai_msgs[0].role, "system");
        assert_eq!(openai_msgs[0].content, Some("You are helpful.".to_string()));
        assert_eq!(openai_msgs[1].role, "user");
        assert_eq!(openai_msgs[2].role, "assistant");
    }

    #[test]
    fn test_tool_call_message_conversion() {
        let tool_calls = vec![ToolCallInfo {
            id: "call_123".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "test.txt"}),
        }];

        let messages = vec![Message::assistant_with_tool_calls(
            "I'll read the file".to_string(),
            tool_calls,
        )];

        let openai_msgs = messages_to_openai(&messages, "");

        assert_eq!(openai_msgs.len(), 1);
        assert_eq!(openai_msgs[0].role, "assistant");
        assert!(openai_msgs[0].tool_calls.is_some());

        let tc = openai_msgs[0].tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "call_123");
        assert_eq!(tc[0].function.name, "read_file");
    }

    #[test]
    fn test_tool_result_conversion() {
        let messages = vec![Message::tool_result(
            "call_123".to_string(),
            "read_file".to_string(),
            "file contents here".to_string(),
        )];

        let openai_msgs = messages_to_openai(&messages, "");

        assert_eq!(openai_msgs.len(), 1);
        assert_eq!(openai_msgs[0].role, "tool");
        assert_eq!(openai_msgs[0].tool_call_id, Some("call_123".to_string()));
        assert_eq!(openai_msgs[0].name, Some("read_file".to_string()));
    }

    #[test]
    fn test_gemini_tools_to_openai() {
        let gemini_tools = vec![serde_json::json!({
            "functionDeclarations": [{
                "name": "read_file",
                "description": "Read a file from disk",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    },
                    "required": ["path"]
                }
            }]
        })];

        let openai_tools = tools_to_openai(&gemini_tools);

        assert_eq!(openai_tools.len(), 1);
        assert_eq!(openai_tools[0].tool_type, "function");
        assert_eq!(openai_tools[0].function.name, "read_file");
        assert_eq!(openai_tools[0].function.description, "Read a file from disk");
    }
}
