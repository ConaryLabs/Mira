// src/llm/intent.rs

use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatIntent {
    pub output: String,
    pub persona: String,
    pub mood: String,
}

impl ChatIntent {
    /// Parse and normalize the ChatIntent from the OpenAI function_call result.
    pub fn from_function_result(raw: &serde_json::Value) -> Self {
        let func_call = &raw["choices"][0]["message"]["function_call"];
        let args = func_call["arguments"].as_str().unwrap_or("{}");

        serde_json::from_str(args).unwrap_or_else(|_| ChatIntent {
            output: "Parse error in function output.".to_string(),
            persona: "Default".to_string(),
            mood: "neutral".to_string(),
        })
    }
}

/// Provides the canonical function schema for intent/persona extraction.
pub fn chat_intent_function_schema() -> serde_json::Value {
    serde_json::json!([{
        "name": "format_response",
        "description": "Return chat as JSON with an 'output' field (the reply), a 'persona' field (overlay: Default, Forbidden, Hallow, or Haven), and a 'mood' field.",
        "parameters": {
            "type": "object",
            "properties": {
                "output": { "type": "string" },
                "persona": {
                    "type": "string",
                    "enum": ["Default", "Forbidden", "Hallow", "Haven"],
                    "description": "Which persona overlay was used for this response"
                },
                "mood": {
                    "type": "string",
                    "description": "The emotional tone (e.g., 'flirty', 'horny', 'soothing', etc.)"
                }
            },
            "required": ["output", "persona", "mood"]
        }
    }])
}
