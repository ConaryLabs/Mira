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
    pub salience: f64,                     // Changed from f32 - SQLite REAL = f64
    pub topics: Vec<String>,
    pub contains_code: bool,
    pub routed_to_heads: Vec<String>,
    #[serde(default = "default_language")]
    pub language: String,
    pub mood: Option<String>,
    pub intensity: Option<f64>,            // Changed from f32 - SQLite REAL = f64
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
    pub prompt_tokens: Option<i64>,        // Changed from i32 - SQLite INTEGER = i64
    pub completion_tokens: Option<i64>,    // Changed from i32
    pub reasoning_tokens: Option<i64>,     // Changed from i32
    pub total_tokens: Option<i64>,         // Changed from i32
    pub finish_reason: Option<String>,
    pub latency_ms: i64,                   // Changed from i32
    pub model_version: String,
    pub temperature: f64,                  // Changed from f32 - SQLite REAL = f64
    pub max_tokens: i64,                   // Changed from i32
    pub reasoning_effort: String,
    pub verbosity: String,
}

#[derive(Debug, Clone)]
pub struct CompleteResponse {
    pub structured: StructuredGPT5Response,
    pub metadata: GPT5Metadata,
    pub raw_response: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<Value>>,     // For code fixes - contains file updates
}

// Code fix specific types (used by code_fix_processor.rs)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixType {
    CompilerError,
    RuntimeError,
    TypeError,
    ImportError,
    SyntaxError,
    LogicError,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    Primary,
    Import,
    Type,
    Cascade,
}

impl CodeFixResponse {
    pub fn validate(&self) -> Result<(), String> {
        for file in &self.files {
            if file.content.contains("...") && !file.content.contains("\"...\"") {
                let without_strings = remove_string_literals(&file.content);
                if without_strings.contains("...") {
                    return Err(format!("File {} contains code ellipsis (...)", file.path));
                }
            }
            
            let incomplete_patterns = [
                "// rest",
                "// unchanged",
                "// existing",
                "// previous",
                "/* ... */",
                "// ...",
                "# rest",
                "# unchanged",
                "// omitted",
                "# omitted",
            ];
            
            for pattern in &incomplete_patterns {
                if file.content.to_lowercase().contains(pattern) {
                    return Err(format!("File {} contains incomplete pattern: {}", file.path, pattern));
                }
            }
            
            let line_count = file.content.lines().count();
            if line_count < 3 {
                tracing::warn!("File {} seems too short ({} lines)", file.path, line_count);
            }
        }
        
        if self.confidence < 0.3 {
            tracing::warn!("Low confidence fix: {}", self.confidence);
        }
        
        let has_primary = self.files.iter().any(|f| matches!(f.change_type, ChangeType::Primary));
        if !has_primary {
            return Err("No primary file marked in fix".to_string());
        }
        
        Ok(())
    }
    
    pub fn validate_line_counts(&self, original_line_count: usize) -> Vec<String> {
        let mut warnings = Vec::new();
        
        for file in &self.files {
            if matches!(file.change_type, ChangeType::Primary) {
                let fixed_lines = file.content.lines().count();
                
                if original_line_count > 0 && fixed_lines < (original_line_count / 2) {
                    warnings.push(format!(
                        "Fixed file {} has {} lines vs original {} lines (less than 50%)",
                        file.path, fixed_lines, original_line_count
                    ));
                }
                
                if original_line_count > 0 && fixed_lines > (original_line_count * 2) {
                    warnings.push(format!(
                        "Fixed file {} has {} lines vs original {} lines (more than 200%)",
                        file.path, fixed_lines, original_line_count
                    ));
                }
            }
        }
        
        warnings
    }
    
    pub fn into_complete_response(self, metadata: GPT5Metadata, raw: Value) -> CompleteResponse {
        CompleteResponse {
            structured: StructuredGPT5Response {
                output: self.output.clone(),
                analysis: self.analysis.clone(),
                reasoning: self.reasoning.clone(),
                schema_name: Some("code_fix_response".to_string()),
                validation_status: Some(self.validate().map(|_| "valid".to_string()).unwrap_or_else(|e| format!("invalid: {}", e))),
            },
            metadata,
            raw_response: raw,
            artifacts: Some(self.files.into_iter().map(|f| serde_json::json!({
                "path": f.path,
                "content": f.content,
                "change_type": f.change_type,
                "original_content": f.original_content,
            })).collect()),
        }
    }
}

fn remove_string_literals(code: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;
    let mut escape_next = false;
    let mut string_char = ' ';
    
    for ch in code.chars() {
        if escape_next {
            result.push(' ');
            escape_next = false;
            continue;
        }
        
        if ch == '\\' && in_string {
            escape_next = true;
            result.push(' ');
            continue;
        }
        
        if !in_string && (ch == '"' || ch == '\'') {
            in_string = true;
            string_char = ch;
            result.push(' ');
        } else if in_string && ch == string_char {
            in_string = false;
            result.push(' ');
        } else if in_string {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    
    result
}
