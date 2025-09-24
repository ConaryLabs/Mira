// src/llm/structured/code_fix_processor.rs

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

// ============================================================================
// TYPES - Code fix specific structures
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeFixResponse {
    pub output: String,           // User-facing explanation
    pub analysis: MessageAnalysis, // Reuse existing analysis
    pub reasoning: Option<String>,
    pub fix_type: FixType,
    pub files: Vec<FileUpdate>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUpdate {
    pub path: String,
    pub content: String,         // COMPLETE file content
    pub change_type: ChangeType,
    pub original_content: Option<String>, // For undo
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
    Primary,  // The file with the error
    Import,   // Files that need import updates
    Type,     // Type definition files
    Cascade,  // Other affected files
}

// Reuse the existing MessageAnalysis from types.rs
use super::types::{MessageAnalysis, CompleteResponse};
use super::processor::extract_metadata;

// ============================================================================
// ERROR DETECTION
// ============================================================================

#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub error_type: String,
    pub file_path: String,
    pub line_number: Option<usize>,
    pub error_message: String,
    pub language: Option<String>,
    pub original_line_count: usize,  // For validation
}

pub fn detect_error_context(content: &str) -> Option<ErrorContext> {
    // Rust compiler errors
    if let Some(captures) = regex::Regex::new(
        r"error\[E\d+\].*?\n\s*--> ([^:]+):(\d+):(\d+)"
    ).ok()?.captures(content) {
        return Some(ErrorContext {
            error_type: "rust_compiler".to_string(),
            file_path: captures.get(1)?.as_str().to_string(),
            line_number: captures.get(2)?.as_str().parse().ok(),
            error_message: content.to_string(),
            language: Some("rust".to_string()),
            original_line_count: 0, // Will be filled when file is loaded
        });
    }
    
    // TypeScript/JavaScript errors
    if let Some(captures) = regex::Regex::new(
        r"(TS|JS)\d+:.*?\n\s*(?:-->)?\s*([^:]+):(\d+):(\d+)"
    ).ok()?.captures(content) {
        return Some(ErrorContext {
            error_type: "typescript".to_string(),
            file_path: captures.get(2)?.as_str().to_string(),
            line_number: captures.get(3)?.as_str().parse().ok(),
            error_message: content.to_string(),
            language: Some("typescript".to_string()),
            original_line_count: 0,
        });
    }
    
    // Python tracebacks
    if content.contains("Traceback (most recent call last)") {
        if let Some(captures) = regex::Regex::new(
            r#"File "([^"]+)", line (\d+)"#
        ).ok()?.captures(content) {
            return Some(ErrorContext {
                error_type: "python_runtime".to_string(),
                file_path: captures.get(1)?.as_str().to_string(),
                line_number: captures.get(2)?.as_str().parse().ok(),
                error_message: content.to_string(),
                language: Some("python".to_string()),
                original_line_count: 0,
            });
        }
    }
    
    // Go compiler errors
    if let Some(captures) = regex::Regex::new(
        r"([^:]+\.go):(\d+):(\d+): (.+)"
    ).ok()?.captures(content) {
        return Some(ErrorContext {
            error_type: "go_compiler".to_string(),
            file_path: captures.get(1)?.as_str().to_string(),
            line_number: captures.get(2)?.as_str().parse().ok(),
            error_message: content.to_string(),
            language: Some("go".to_string()),
            original_line_count: 0,
        });
    }
    
    // Generic error pattern
    if let Some(captures) = regex::Regex::new(
        r"(?i)error.*?(?:in|at)\s+([^\s:]+(?:\.\w+)?):?(\d+)?"
    ).ok()?.captures(content) {
        return Some(ErrorContext {
            error_type: "generic".to_string(),
            file_path: captures.get(1)?.as_str().to_string(),
            line_number: captures.get(2).and_then(|m| m.as_str().parse().ok()),
            error_message: content.to_string(),
            language: None,
            original_line_count: 0,
        });
    }
    
    None
}

// ============================================================================
// REQUEST BUILDING (just the schema, prompts go in unified_builder.rs)
// ============================================================================

pub fn get_code_fix_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "output": {
                "type": "string",
                "description": "Clear explanation of what was fixed and why"
            },
            "analysis": {
                "type": "object",
                "properties": {
                    "salience": {"type": "number", "minimum": 0, "maximum": 10},
                    "topics": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "mood": {"type": ["string", "null"]},
                    "intent": {"type": ["string", "null"]}
                },
                "required": ["salience", "topics"]
            },
            "reasoning": {
                "type": ["string", "null"],
                "description": "Step-by-step reasoning about the fix"
            },
            "fix_type": {
                "type": "string",
                "enum": ["compiler_error", "runtime_error", "type_error", "import_error", "syntax_error", "logic_error", "other"]
            },
            "files": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path relative to project root"
                        },
                        "content": {
                            "type": "string",
                            "description": "COMPLETE file content from first line to last line, no abbreviations"
                        },
                        "change_type": {
                            "type": "string",
                            "enum": ["primary", "import", "type", "cascade"],
                            "description": "Type of change in this file"
                        }
                    },
                    "required": ["path", "content", "change_type"]
                },
                "minItems": 1,
                "description": "Complete files to replace existing ones"
            },
            "confidence": {
                "type": "number",
                "minimum": 0,
                "maximum": 1,
                "description": "Confidence in the fix (0-1)"
            }
        },
        "required": ["output", "analysis", "fix_type", "files", "confidence"],
        "additionalProperties": false
    })
}

// ============================================================================
// RESPONSE VALIDATION
// ============================================================================

impl CodeFixResponse {
    pub fn validate(&self) -> Result<(), String> {
        // Check each file for completeness
        for file in &self.files {
            // Check for ellipsis patterns
            if file.content.contains("...") && !file.content.contains("...") {
                // Allow "..." in strings but not as code ellipsis
                let without_strings = remove_string_literals(&file.content);
                if without_strings.contains("...") {
                    return Err(format!("File {} contains code ellipsis (...)", file.path));
                }
            }
            
            // Check for common incomplete patterns
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
            
            // Validate minimum content length
            let line_count = file.content.lines().count();
            if line_count < 3 {
                warn!("File {} seems too short ({} lines)", file.path, line_count);
            }
        }
        
        // Validate confidence
        if self.confidence < 0.3 {
            warn!("Low confidence fix: {}", self.confidence);
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
            structured: super::types::StructuredGPT5Response {
                output: self.output.clone(),
                analysis: self.analysis.clone(),
                reasoning: self.reasoning.clone(),
                schema_name: Some("code_fix_response".to_string()),
                validation_status: self.validate().map(|_| "valid").unwrap_or("invalid").to_string(),
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

// ============================================================================
// EXTRACTION
// ============================================================================

pub fn extract_code_fix_response(raw_response: &Value) -> Result<CodeFixResponse> {
    // Extract the content from the response
    let content = raw_response["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in response"))?;
    
    // Parse JSON
    let parsed: CodeFixResponse = serde_json::from_str(content)?;
    
    // Validate
    parsed.validate()
        .map_err(|e| anyhow::anyhow!("Validation failed: {}", e))?;
    
    // Log file stats
    for file in &parsed.files {
        let line_count = file.content.lines().count();
        let byte_count = file.content.len();
        info!("File {} has {} lines, {} bytes, type: {:?}", 
              file.path, line_count, byte_count, file.change_type);
    }
    
    Ok(parsed)
}

// ============================================================================
// UTILITIES
// ============================================================================

/// Load complete file content from project
pub async fn load_complete_file(
    project_path: &str,
    file_path: &str,
) -> Result<(String, usize)> {
    use tokio::fs;
    use std::path::Path;
    
    let full_path = Path::new(project_path).join(file_path);
    
    let content = fs::read_to_string(&full_path).await
        .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", full_path.display(), e))?;
    
    let line_count = content.lines().count();
    
    info!("Loaded {} ({} lines, {} bytes)", 
          file_path, 
          line_count, 
          content.len());
    
    Ok((content, line_count))
}

/// Remove string literals for better pattern detection
fn remove_string_literals(code: &str) -> String {
    // Simple approach: replace string content with spaces
    // This prevents false positives from "..." in strings
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
