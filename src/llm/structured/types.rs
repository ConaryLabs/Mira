// src/llm/structured/types.rs
// Complete structured response types for data-hoarding machine

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Complete structured response with ALL database fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredGPT5Response {
    /// Core content for memory_entries.content
    pub output: String,
    
    /// Complete analysis for message_analysis table
    pub analysis: MessageAnalysis,
    
    /// Optional reasoning for debugging
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

/// Maps exactly to message_analysis table - every field accounted for
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAnalysis {
    // REQUIRED fields (will fail if missing)
    pub salience: f64,                    // 0-10 CHECK constraint - REAL in DB
    pub topics: Vec<String>,              // At least 1 required
    pub contains_code: bool,              // Triggers code intelligence
    pub routed_to_heads: Vec<String>,     // At least 1 required
    #[serde(default = "default_language")]
    pub language: String,                 // Default "en"
    
    // OPTIONAL fields (but captured for completeness)
    pub mood: Option<String>,
    pub intensity: Option<f64>,           // 0-1 CHECK constraint - REAL in DB
    pub intent: Option<String>,
    pub summary: Option<String>,
    pub relationship_impact: Option<String>,
    pub programming_lang: Option<String>, // Required if contains_code=true
}

/// Metadata extracted from raw GPT-5 response
#[derive(Debug, Clone)]
pub struct GPT5Metadata {
    // From response envelope
    pub response_id: Option<String>,
    pub prompt_tokens: Option<i64>,       // INTEGER in DB
    pub completion_tokens: Option<i64>,   // INTEGER in DB
    pub reasoning_tokens: Option<i64>,    // INTEGER in DB
    pub total_tokens: Option<i64>,        // INTEGER in DB
    pub finish_reason: Option<String>,
    
    // Measured by us
    pub latency_ms: i64,                  // INTEGER in DB
    
    // From CONFIG
    pub model_version: String,
    pub temperature: f64,                 // REAL in DB
    pub max_tokens: i64,                  // INTEGER in DB
    pub reasoning_effort: String,
    pub verbosity: String,
}

/// Complete response with all metadata - the holy grail
#[derive(Debug, Clone)]
pub struct CompleteResponse {
    pub structured: StructuredGPT5Response,
    pub metadata: GPT5Metadata,
    pub raw_response: Value,
}

fn default_language() -> String {
    "en".to_string()
}
