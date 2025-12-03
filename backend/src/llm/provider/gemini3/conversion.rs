// src/llm/provider/gemini3/conversion.rs
// Message and tool format conversion for Gemini API

use serde_json::Value;
use crate::llm::provider::Message;

/// Convert our Message format to Gemini API format
pub fn messages_to_gemini_contents(messages: &[Message], system: &str) -> Vec<Value> {
    let mut contents = Vec::new();

    // Add system instruction as first user message if present
    let system_text = if !system.is_empty() {
        Some(system.to_string())
    } else {
        None
    };

    let mut system_added = false;

    for msg in messages {
        let role = match msg.role.as_str() {
            "user" => "user",
            "assistant" => "model",
            "tool" => "function",
            "system" => continue, // Skip system messages, handled separately
            _ => "user",
        };

        let mut parts = Vec::new();

        // Add system instruction to first user message
        if role == "user" && !system_added {
            if let Some(ref sys) = system_text {
                parts.push(serde_json::json!({"text": format!("[System]\n{}\n\n[User]\n", sys)}));
            }
            system_added = true;
        }

        // Handle function responses
        if msg.role == "tool" {
            if let Some(ref call_id) = msg.tool_call_id {
                contents.push(serde_json::json!({
                    "role": "function",
                    "parts": [{
                        "functionResponse": {
                            "name": call_id,
                            "response": {
                                "result": msg.content
                            }
                        }
                    }]
                }));
                continue;
            }
        }

        // Add text content
        if !msg.content.is_empty() {
            parts.push(serde_json::json!({"text": msg.content}));
        }

        // Add thought signature if present
        if let Some(ref sig) = msg.thought_signature {
            parts.push(serde_json::json!({"thoughtSignature": sig}));
        }

        // Add function calls if present (for model messages)
        if let Some(ref tool_calls) = msg.tool_calls {
            for tc in tool_calls {
                parts.push(serde_json::json!({
                    "functionCall": {
                        "name": tc.name,
                        "args": tc.arguments
                    }
                }));
            }
        }

        if !parts.is_empty() {
            contents.push(serde_json::json!({
                "role": role,
                "parts": parts
            }));
        }
    }

    // If system wasn't added (no user messages), add it as first message
    if !system_added && system_text.is_some() {
        contents.insert(
            0,
            serde_json::json!({
                "role": "user",
                "parts": [{"text": system_text.unwrap()}]
            }),
        );
    }

    contents
}

/// Convert OpenAI-format tools to Gemini format
pub fn tools_to_gemini_format(tools: &[Value]) -> Value {
    let function_declarations: Vec<Value> = tools
        .iter()
        .filter_map(|tool| {
            // Handle OpenAI format: { type: "function", function: { name, description, parameters } }
            if let Some(func) = tool.get("function") {
                Some(serde_json::json!({
                    "name": func.get("name"),
                    "description": func.get("description"),
                    "parameters": func.get("parameters")
                }))
            } else if tool.get("name").is_some() {
                // Already in simple format
                Some(tool.clone())
            } else {
                None
            }
        })
        .collect();

    serde_json::json!({
        "functionDeclarations": function_declarations
    })
}
