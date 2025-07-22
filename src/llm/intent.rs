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
        // Try to get the function call from the response
        let func_call = raw
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("function_call"));
        
        if let Some(fc) = func_call {
            let args = fc.get("arguments")
                .and_then(|a| a.as_str())
                .unwrap_or("{}");
            
            match serde_json::from_str::<ChatIntent>(args) {
                Ok(intent) => intent,
                Err(_) => {
                    // Try to get content from regular message
                    let content = raw
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|choice| choice.get("message"))
                        .and_then(|msg| msg.get("content"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("I'm having trouble formulating my response.");
                    
                    ChatIntent {
                        output: content.to_string(),
                        persona: "Default".to_string(),
                        mood: "confused".to_string(),
                    }
                }
            }
        } else {
            // No function call found, try to get regular content
            let content = raw
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|choice| choice.get("message"))
                .and_then(|msg| msg.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("I couldn't process my response properly.");
            
            ChatIntent {
                output: content.to_string(),
                persona: "Default".to_string(),
                mood: "uncertain".to_string(),
            }
        }
    }
}

/// Provides the canonical function schema for intent/persona extraction.
pub fn chat_intent_function_schema() -> serde_json::Value {
    serde_json::json!({
        "name": "format_response",
        "description": "Format Mira's response with persona and mood information",
        "parameters": {
            "type": "object",
            "properties": {
                "output": { 
                    "type": "string",
                    "description": "Mira's actual response to the user"
                },
                "persona": {
                    "type": "string",
                    "enum": ["Default", "Forbidden", "Hallow", "Haven"],
                    "description": "Which persona overlay was used for this response"
                },
                "mood": {
                    "type": "string",
                    "description": "The emotional tone of the response (e.g., 'playful', 'caring', 'sassy', 'thoughtful')"
                }
            },
            "required": ["output", "persona", "mood"]
        }
    })
}
