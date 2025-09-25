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
    // FIXED: Properly format the string with placeholders
    let enhanced_prompt = format!(
        "{}\n\nCRITICAL CODE FIX REQUIREMENTS:\n\
        1. Provide COMPLETE files from line 1 to last line\n\
        2. NEVER use '...' or '// rest unchanged'\n\
        3. Include ALL imports, functions, code\n\
        4. System will REPLACE ENTIRE FILES\n\
        \n\
        Error: {}\n\
        File: {}\n\
        \n\
        Complete file content:\n\
        ```\n\
        {}\n\
        ```",
        system_prompt, error_message, file_path, file_content
    );
    
    let response_schema = json!({
        "type": "object",
        "properties": {
            "output": {"type": "string"},
            "analysis": {"$ref": "#/definitions/MessageAnalysis"},
            "reasoning": {"type": "string"},
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
        "required": ["output", "analysis", "fix_type", "files", "confidence"]
    });
    
    let mut messages = vec![
        json!({"role": "system", "content": enhanced_prompt})
    ];
    messages.extend(context_messages);
    messages.push(json!({"role": "user", "content": error_message}));
    
    Ok(json!({
        "messages": messages,
        "model": "gpt-5",
        "response_format": {
            "type": "json_schema",
            "json_schema": response_schema
        },
        "stream": false
    }))
}

// ============================================================================
// RESPONSE EXTRACTION
// ============================================================================

pub fn extract_code_fix_response(raw_response: &Value) -> Result<CodeFixResponse> {
    // Extract the content from the response
    let content = raw_response["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in response"))?;
    
    // Parse the JSON content
    let parsed: Value = serde_json::from_str(content)?;
    
    // Convert to CodeFixResponse
    let response = serde_json::from_value::<CodeFixResponse>(parsed)?;
    
    // Validate the response - FIXED: Convert String error to anyhow::Error
    response.validate()
        .map_err(|e| anyhow::anyhow!("Validation failed: {}", e))?;
    
    Ok(response)
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
        CompleteResponse {
            structured: StructuredGPT5Response {
                output: self.output.clone(),
                analysis: self.analysis.clone(),
                reasoning: self.reasoning.clone(),
                schema_name: Some("code_fix_response".to_string()),
                validation_status: Some(
                    self.validate()
                        .map(|_| "valid".to_string())
                        .unwrap_or_else(|_| "invalid".to_string())
                ),
            },
            metadata,
            raw_response: raw,
            // Include artifacts for the frontend
            artifacts: Some(self.files.into_iter().map(|f| json!({
                "path": f.path,
                "content": f.content,
                "change_type": f.change_type,
            })).collect()),
        }
    }
}
