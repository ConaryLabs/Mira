// src/llm/structured/code_fix_processor.rs

use anyhow::Result;
use serde_json::{json, Value};
use regex::Regex;
use super::types::{CodeFixResponse, ChangeType, CompleteResponse, StructuredGPT5Response};

// ============================================================================
// ERROR DETECTION
// ============================================================================

#[derive(Debug)]
pub struct ErrorContext {
    pub error_type: String,
    pub file_path: String,
    pub error_message: String,
    pub line_number: Option<usize>,
    pub original_line_count: usize,
}

pub fn detect_error_context(content: &str) -> Option<ErrorContext> {
    let patterns = vec![
        (r"error\[E(\d+)\]:\s*(.*)", "rust_compiler"),
        (r"TS(\d+):\s*(.*)", "typescript"),
        (r"TypeError:\s*(.*)", "javascript_type"),
        (r"thread '.*' panicked", "rust_panic"),
    ];
    
    for (pattern_str, error_type) in patterns {
        if let Ok(regex) = Regex::new(pattern_str) {
            if regex.is_match(content) {
                if let Some(file_path) = extract_file_path(content) {
                    return Some(ErrorContext {
                        error_type: error_type.to_string(),
                        file_path,
                        error_message: content.to_string(),
                        line_number: extract_line_number(content),
                        original_line_count: 0, // Will be filled when file is loaded
                    });
                }
            }
        }
    }
    None
}

fn extract_file_path(error_msg: &str) -> Option<String> {
    let patterns = vec![
        r"(?:-->|at|in)\s+([^\s:]+(?:\.rs|\.ts|\.tsx|\.js|\.jsx))(?::|$)",
        r"([^\s]+(?:\.rs|\.ts|\.tsx|\.js|\.jsx)):\d+:\d+",
    ];
    
    for pattern_str in patterns {
        if let Ok(regex) = Regex::new(pattern_str) {
            if let Some(captures) = regex.captures(error_msg) {
                if let Some(path) = captures.get(1) {
                    return Some(path.as_str().to_string());
                }
            }
        }
    }
    None
}

fn extract_line_number(error_msg: &str) -> Option<usize> {
    Regex::new(r":(\d+):\d+").ok()?
        .captures(error_msg)?
        .get(1)?
        .as_str()
        .parse()
        .ok()
}

// ============================================================================
// REQUEST BUILDING
// ============================================================================

pub fn build_code_fix_request(
    error_message: &str,
    file_path: &str,
    file_content: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
    use crate::CONFIG;
    
    // Build full input string (GPT-5 Responses API format)
    let mut full_input = format!(
        "{}\n\nCRITICAL CODE FIX REQUIREMENTS:\n\
        1. Provide COMPLETE files from line 1 to last line\n\
        2. NEVER use '...' or '// rest unchanged'\n\
        3. Include ALL imports, functions, code\n\
        4. System will REPLACE ENTIRE FILES\n\
        \n\
        Error: {}\n\
        File: {}\n\
        Original file has {} lines.\n\
        \n\
        Complete file content:\n\
```\n\
        {}\n\
```",
        system_prompt, error_message, file_path, 
        file_content.lines().count(),
        file_content
    );
    
    // Add context messages
    for msg in context_messages {
        if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
            if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                full_input.push_str(&format!("\n\n{}: {}", role, content));
            }
        }
    }
    
    full_input.push_str(&format!("\n\nUser: Fix this error: {}", error_message));
    
    let response_schema = json!({
        "type": "object",
        "properties": {
            "output": {"type": "string"},
            "analysis": {
                "type": "object",
                "properties": {
                    "salience": {"type": "number", "minimum": 0.0, "maximum": 10.0},
                    "topics": {"type": "array", "items": {"type": "string"}, "minItems": 1},
                    "contains_code": {"type": "boolean"},
                    "routed_to_heads": {"type": "array", "items": {"type": "string"}, "minItems": 1},
                    "language": {"type": "string", "default": "en"},
                    "mood": {"type": ["string", "null"]},
                    "intensity": {"type": ["number", "null"]},
                    "intent": {"type": ["string", "null"]},
                    "summary": {"type": ["string", "null"]},
                    "relationship_impact": {"type": ["string", "null"]},
                    "programming_lang": {"type": ["string", "null"]}
                },
                "required": ["salience", "topics", "contains_code", "routed_to_heads", "language", 
                           "mood", "intensity", "intent", "summary", "relationship_impact", "programming_lang"],
                "additionalProperties": false
            },
            "reasoning": {"type": ["string", "null"]},
            "fix_type": {
                "type": "string",
                "enum": ["compiler_error", "runtime_error", "type_error", "import_error", "syntax_error"]
            },
            "files": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"},
                        "change_type": {
                            "type": "string",
                            "enum": ["primary", "import", "type", "cascade"]
                        }
                    },
                    "required": ["path", "content", "change_type"]
                }
            },
            "confidence": {"type": "number"}
        },
        "required": ["output", "analysis", "reasoning", "fix_type", "files", "confidence"],
        "additionalProperties": false
    });
    
    // GPT-5 Responses API format
    Ok(json!({
        "model": CONFIG.gpt5_model,
        "input": full_input,
        "text": {
            "verbosity": CONFIG.verbosity,
            "format": {
                "type": "json_schema",
                "name": "code_fix_response",
                "schema": response_schema,
                "strict": true
            }
        },
        "reasoning": {
            "effort": CONFIG.reasoning_effort
        }
    }))
}

// ============================================================================
// RESPONSE EXTRACTION
// ============================================================================

pub fn extract_code_fix_response(raw_response: &Value) -> Result<CodeFixResponse> {
    // Handle GPT-5 Responses API array format
    if let Some(output_array) = raw_response.get("output").and_then(|v| v.as_array()) {
        for item in output_array {
            if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                    for content_item in content_array {
                        if content_item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                            if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                                let response = serde_json::from_str::<CodeFixResponse>(text)?;
                                response.validate()
                                    .map_err(|e| anyhow::anyhow!("Validation failed: {}", e))?;
                                return Ok(response);
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Fallback: try direct text field
    if let Some(text_field) = raw_response.get("text").and_then(|v| v.as_str()) {
        let response = serde_json::from_str::<CodeFixResponse>(text_field)?;
        response.validate()
            .map_err(|e| anyhow::anyhow!("Validation failed: {}", e))?;
        return Ok(response);
    }
    
    // Fallback: try output field as string
    if let Some(output) = raw_response.get("output").and_then(|v| v.as_str()) {
        let response = serde_json::from_str::<CodeFixResponse>(output)?;
        response.validate()
            .map_err(|e| anyhow::anyhow!("Validation failed: {}", e))?;
        return Ok(response);
    }
    
    Err(anyhow::anyhow!("Could not extract code fix response from GPT-5 output"))
}

// ============================================================================
// VALIDATION & CONVERSION
// ============================================================================

impl CodeFixResponse {
    pub fn validate(&self) -> Result<(), String> {
        // Check for forbidden patterns
        for file in &self.files {
            if file.content.contains("...") {
                return Err(format!("File {} contains ellipsis", file.path));
            }
            if file.content.contains("// rest") || file.content.contains("// unchanged") {
                return Err(format!("File {} contains snippets", file.path));
            }
        }
        
        // Check confidence
        if self.confidence < 0.5 {
            tracing::warn!("Low confidence fix: {}", self.confidence);
        }
        
        // Validate at least one primary file
        let has_primary = self.files.iter().any(|f| matches!(f.change_type, ChangeType::Primary));
        if !has_primary {
            return Err("No primary file marked in fix".to_string());
        }
        
        Ok(())
    }
    
    pub fn validate_line_counts(&self, error_context: &ErrorContext) -> Vec<String> {
        let mut warnings = Vec::new();
        
        // Check the primary file's line count
        for file in &self.files {
            if matches!(file.change_type, ChangeType::Primary) {
                let fixed_lines = file.content.lines().count();
                let original_lines = error_context.original_line_count;
                
                // If the fixed file is less than 50% of original, it's probably incomplete
                if original_lines > 0 && fixed_lines < (original_lines / 2) {
                    warnings.push(format!(
                        "Fixed file {} has {} lines vs original {} lines (less than 50%)",
                        file.path, fixed_lines, original_lines
                    ));
                }
                
                // If it's more than 200% of original, might be duplicated
                if original_lines > 0 && fixed_lines > (original_lines * 2) {
                    warnings.push(format!(
                        "Fixed file {} has {} lines vs original {} lines (more than 200%)",
                        file.path, fixed_lines, original_lines
                    ));
                }
            }
        }
        
        warnings
    }
    
    pub fn into_complete_response(self, metadata: super::types::GPT5Metadata, raw: Value) -> CompleteResponse {
        // Convert files to artifacts for frontend
        let artifacts = Some(self.files.iter().map(|f| json!({
            "path": f.path.clone(),
            "content": f.content.clone(),
            "change_type": match f.change_type {
                ChangeType::Primary => "primary",
                ChangeType::Import => "import",
                ChangeType::Type => "type",
                ChangeType::Cascade => "cascade",
            },
        })).collect());
        
        CompleteResponse {
            structured: StructuredGPT5Response {
                output: self.output.clone(),
                analysis: self.analysis.clone(),
                reasoning: self.reasoning.clone(),
                schema_name: Some("code_fix_response".to_string()),
                validation_status: Some(
                    self.validate()
                        .map(|_| "valid".to_string())
                        .unwrap_or_else(|e| format!("invalid: {}", e))
                ),
            },
            metadata,
            raw_response: raw,
            artifacts,
        }
    }
}
