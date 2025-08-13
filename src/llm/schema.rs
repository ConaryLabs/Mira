// src/llm/schema.rs
// Phase 4: Types and function schemas for GPT-5 Functions API
// Provides structured types for memory evaluation and chat responses

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Request to evaluate a message for memory metadata using Functions API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateMemoryRequest {
    pub content: String,           // The message content to analyze
    #[serde(skip)]
    pub function_schema: Value,    // The function schema (computed, not serialized)
}

impl EvaluateMemoryRequest {
    /// Create a new evaluation request with the standard schema
    pub fn new(content: String) -> Self {
        Self {
            content,
            function_schema: function_schema(),
        }
    }
}

/// Structured response from the GPT-5 function call for memory evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateMemoryResponse {
    pub salience: u8,              // 1-10 emotional importance
    pub tags: Vec<String>,         // Context and mood tags
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,   // Optional one-sentence summary
    pub memory_type: MemoryType,   // Categorization of the memory
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Feeling,
    Fact,
    Joke,
    Promise,
    Event,
    #[serde(other)]
    Other,
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryType::Feeling => write!(f, "feeling"),
            MemoryType::Fact => write!(f, "fact"),
            MemoryType::Joke => write!(f, "joke"),
            MemoryType::Promise => write!(f, "promise"),
            MemoryType::Event => write!(f, "event"),
            MemoryType::Other => write!(f, "other"),
        }
    }
}

/// Returns the JSON schema for the "evaluate_memory" function
/// Compatible with GPT-5 Functions API specification
pub fn function_schema() -> Value {
    json!({
        "name": "evaluate_memory",
        "description": "Evaluates a chat message for emotional significance, categorization, and contextual tags to determine storage priority and retrieval relevance.",
        "parameters": {
            "type": "object",
            "properties": {
                "salience": {
                    "type": "integer",
                    "description": "Emotional importance of this message on a scale of 1-10. Higher values indicate messages that are more meaningful, surprising, or emotionally charged.",
                    "minimum": 1,
                    "maximum": 10
                },
                "tags": {
                    "type": "array",
                    "description": "Contextual tags describing the mood, topic, and relationship aspects of this memory. Include emotional tone, subject matter, and any notable themes.",
                    "items": { 
                        "type": "string",
                        "maxLength": 50
                    },
                    "minItems": 1,
                    "maxItems": 10
                },
                "summary": {
                    "type": ["string", "null"],
                    "description": "A concise one-sentence summary capturing the essence of this memory. Omit if the message is self-explanatory.",
                    "maxLength": 200
                },
                "memory_type": {
                    "type": "string",
                    "enum": ["feeling", "fact", "joke", "promise", "event", "other"],
                    "description": "Classification of the memory type: feeling (emotional state), fact (information), joke (humor), promise (commitment), event (happening), or other."
                }
            },
            "required": ["salience", "tags", "memory_type"],
            "additionalProperties": false
        }
    })
}

/// Main chat response structure from Mira
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monologue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aside_intensity: Option<u8>,
}

/// Structured reply format that Mira uses internally
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiraStructuredReply {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monologue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aside_intensity: Option<u8>,
}

impl Default for ChatResponse {
    fn default() -> Self {
        Self {
            output: String::new(),
            persona: "assistant".to_string(),
            mood: "neutral".to_string(),
            salience: 5,
            summary: None,
            memory_type: "other".to_string(),
            tags: vec![],
            intent: "response".to_string(),
            monologue: None,
            reasoning_summary: None,
            aside_intensity: None,
        }
    }
}

impl Default for MiraStructuredReply {
    fn default() -> Self {
        Self {
            output: String::new(),
            persona: "assistant".to_string(),
            mood: "neutral".to_string(),
            salience: 5,
            summary: None,
            memory_type: "other".to_string(),
            tags: vec![],
            intent: "response".to_string(),
            monologue: None,
            reasoning_summary: None,
            aside_intensity: None,
        }
    }
}

/// Helper function to create evaluation request with default schema
pub fn create_evaluation_request(content: String) -> EvaluateMemoryRequest {
    EvaluateMemoryRequest::new(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_schema_structure() {
        let schema = function_schema();
        
        // Verify the schema has the required fields
        assert_eq!(schema["name"], "evaluate_memory");
        assert!(schema["description"].is_string());
        assert!(schema["parameters"].is_object());
        
        let params = &schema["parameters"];
        assert_eq!(params["type"], "object");
        assert!(params["properties"].is_object());
        assert!(params["required"].is_array());
    }

    #[test]
    fn test_memory_type_serialization() {
        assert_eq!(serde_json::to_string(&MemoryType::Feeling).unwrap(), "\"feeling\"");
        assert_eq!(serde_json::to_string(&MemoryType::Event).unwrap(), "\"event\"");
        
        let parsed: MemoryType = serde_json::from_str("\"fact\"").unwrap();
        assert_eq!(parsed, MemoryType::Fact);
    }

    #[test]
    fn test_evaluation_response_parsing() {
        let json_str = r#"{
            "salience": 8,
            "tags": ["important", "emotional"],
            "memory_type": "feeling",
            "summary": "User expressed strong emotions"
        }"#;
        
        let response: EvaluateMemoryResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.salience, 8);
        assert_eq!(response.tags.len(), 2);
        assert_eq!(response.memory_type, MemoryType::Feeling);
        assert!(response.summary.is_some());
    }
}
