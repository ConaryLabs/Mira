// src/llm/schema.rs

//! Types and function schemas for LLM (GPT-4.1) function-calling.
//! Enforces strict, structured JSON responses for memory evaluation.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Request to evaluate a message for memory metadata (for LLM function-call).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateMemoryRequest {
    pub content: String,           // The user/Mira message to analyze.
    #[serde(skip_serializing)]
    pub function_schema: Value,    // The strict JSON schema for the function-call (see below).
}

/// Structured response from the LLM function-call (metadata for storage).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateMemoryResponse {
    pub salience: u8,              // 1-10
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub memory_type: MemoryType,
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

/// Returns the JSON schema for the "evaluate_memory" function-call.
/// This matches what GPT-4.1 expects for strict output parsing.
pub fn function_schema() -> Value {
    json!({
        "name": "evaluate_memory",
        "description": "Evaluates a chat message for emotional significance, tags, summary, and type.",
        "parameters": {
            "type": "object",
            "properties": {
                "salience": {
                    "type": "integer",
                    "description": "How emotionally important is this message? (1-10)",
                    "minimum": 1,
                    "maximum": 10
                },
                "tags": {
                    "type": "array",
                    "description": "Context, mood, and relationship tags for this memory.",
                    "items": { "type": "string" }
                },
                "summary": {
                    "type": ["string", "null"],
                    "description": "A one-sentence summary of this memory (optional)."
                },
                "memory_type": {
                    "type": "string",
                    "enum": ["feeling", "fact", "joke", "promise", "event", "other"],
                    "description": "What kind of memory is this?"
                }
            },
            "required": ["salience", "tags", "memory_type"]
        }
    })
}
