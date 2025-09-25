// src/llm/structured/types.rs

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredGPT5Response {
    pub output: String,
    pub analysis: MessageAnalysis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAnalysis {
    pub salience: f64,
    pub topics: Vec<String>,
    pub contains_code: bool,
    pub routed_to_heads: Vec<String>,
    #[serde(default = "default_language")]
    pub language: String,
    pub mood: Option<String>,
    pub intensity: Option<f64>,
    pub intent: Option<String>,
    pub summary: Option<String>,
    pub relationship_impact: Option<String>,
    pub programming_lang: Option<String>,
}

fn default_language() -> String {
    "en".to_string()
}

#[derive(Debug, Clone)]
pub struct GPT5Metadata {
    pub response_id: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub finish_reason: Option<String>,
    pub latency_ms: i64,
    pub model_version: String,
    pub temperature: f64,
    pub max_tokens: i64,
    pub reasoning_effort: String,
    pub verbosity: String,
}

#[derive(Debug, Clone)]
pub struct CompleteResponse {
    pub structured: StructuredGPT5Response,
    pub metadata: GPT5Metadata,
    pub raw_response: Value,
    pub artifacts: Option<Vec<Value>>,  // For code fixes - contains file updates
}

// Code fix specific types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeFixResponse {
    pub output: String,
    pub analysis: MessageAnalysis,
    pub reasoning: Option<String>,
    pub fix_type: FixType,
    pub files: Vec<FileUpdate>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUpdate {
    pub path: String,
    pub content: String,
    pub change_type: ChangeType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixType {
    CompilerError,
    RuntimeError,
    TypeError,
    ImportError,
    SyntaxError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    Primary,
    Import,
    Type,
    Cascade,
}
