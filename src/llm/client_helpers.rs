// src/llm/client_helpers.rs
// Helper functions for extracting text from GPT-5 responses

use serde_json::Value;

/// Extract text from GPT-5 responses JSON
pub fn extract_text_from_responses(resp_json: &Value) -> Option<String> {
    // Try unified output format first
    if let Some(output) = resp_json.get("output").and_then(|o| o.as_array()) {
        for item in output {
            if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                return Some(text.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Try choices format (older format)
    if let Some(choices) = resp_json.get("choices").and_then(|c| c.as_array()) {
        if let Some(first_choice) = choices.first() {
            if let Some(message) = first_choice.get("message") {
                if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                    return Some(content.to_string());
                }
            }
        }
    }

    None
}
