// src/llm/structured/code_fix_processor.rs
// Code fix request/response handling using Claude with tool calling
// FIXED: Removed overly aggressive generic error detection

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::warn;

use crate::config::CONFIG;
use super::types::{CompleteResponse, LLMMetadata, StructuredLLMResponse, MessageAnalysis};
use super::tool_schema::get_code_fix_tool_schema;
use super::claude_processor::analyze_message_complexity;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeFixFile {
    pub path: String,
    pub content: String,
    pub change_type: ChangeType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    Primary,
    Import,
    Type,
    Cascade,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeFixResponse {
    pub output: String,
    pub analysis: MessageAnalysis,
    pub reasoning: Option<String>,
    pub fix_type: String,
    pub files: Vec<CodeFixFile>,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub error_message: String,
    pub file_path: String,
    pub original_line_count: usize,
}

/// Detects if a message contains a compiler/runtime error
/// Returns ErrorContext if an error is detected
pub fn detect_error_context(message: &str) -> Option<ErrorContext> {
    // Rust compiler errors: error[E0308]: ... --> src/path/file.rs:line:col
    // Must have both the error code AND the arrow pointer
    if let Some(_captures) = regex::Regex::new(r"error\[E\d+\]:")
        .ok()?
        .captures(message) 
    {
        // Extract file path from "--> src/path/file.rs:line:col"
        if let Some(path_match) = regex::Regex::new(r"-->\s+([^\s:]+)")
            .ok()?
            .captures(message)
        {
            let file_path = path_match.get(1)?.as_str().to_string();
            return Some(ErrorContext {
                error_message: message.to_string(),
                file_path,
                original_line_count: 0, // Will be set by handler
            });
        }
    }
    
    // TypeScript/JavaScript errors: path/file.ts(line,col): error TS...
    // Must have the specific format with line numbers and "error TS"
    if let Some(captures) = regex::Regex::new(r"([^\s:]+\.(?:ts|tsx|js|jsx))\(\d+,\d+\):\s+error\s+TS\d+")
        .ok()?
        .captures(message)
    {
        let file_path = captures.get(1)?.as_str().to_string();
        return Some(ErrorContext {
            error_message: message.to_string(),
            file_path,
            original_line_count: 0,
        });
    }
    
    // Python errors: File "path/file.py", line X
    // Must have the specific format with "File" and "line"
    if let Some(captures) = regex::Regex::new(r#"File\s+"([^"]+\.py)",\s+line\s+\d+"#)
        .ok()?
        .captures(message)
    {
        let file_path = captures.get(1)?.as_str().to_string();
        return Some(ErrorContext {
            error_message: message.to_string(),
            file_path,
            original_line_count: 0,
        });
    }
    
    // ESLint errors: path/file.js:line:col - error message (rule-name)
    if let Some(captures) = regex::Regex::new(r"([^\s:]+\.(?:js|jsx|ts|tsx)):(\d+):(\d+)\s+-\s+(.+)")
        .ok()?
        .captures(message)
    {
        let file_path = captures.get(1)?.as_str().to_string();
        return Some(ErrorContext {
            error_message: message.to_string(),
            file_path,
            original_line_count: 0,
        });
    }
    
    // Go compiler errors: path/file.go:line:col: error message
    if let Some(captures) = regex::Regex::new(r"([^\s:]+\.go):(\d+):(\d+):\s+(.+)")
        .ok()?
        .captures(message)
    {
        let file_path = captures.get(1)?.as_str().to_string();
        // Verify it actually says "error" or similar
        let error_text = captures.get(4)?.as_str();
        if error_text.to_lowercase().contains("error") || 
           error_text.to_lowercase().contains("undefined") ||
           error_text.to_lowercase().contains("cannot") {
            return Some(ErrorContext {
                error_message: message.to_string(),
                file_path,
                original_line_count: 0,
            });
        }
    }
    
    // REMOVED: Generic fallback that was causing false positives
    // DO NOT add back a generic "contains error + contains filepath" check
    // It must be a specific compiler error format to trigger
    
    None
}

/// Build code fix request with forced tool choice (no thinking)
/// 
/// Code fixes require deterministic output, which means:
/// - Low/medium temperature (not forced to 1.0)
/// - Forced tool choice for guaranteed structured artifacts
/// - No thinking (incompatible with forced tool + would require temp 1.0)
/// 
/// Trade-off: Less deep reasoning, but more consistent and reliable fixes.
pub fn build_code_fix_request(
    error_message: &str,
    file_path: &str,
    file_content: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
    // Build user message with error context
    let user_message = format!(
        "CRITICAL CODE FIX REQUIREMENTS:\n\
        1. Provide COMPLETE files from line 1 to last line\n\
        2. NEVER use '...' or '// rest unchanged'\n\
        3. Include ALL imports, functions, code\n\
        \n\
        Error: {}\n\
        File: {}\n\
        Original file has {} lines.\n\
        \n\
        Complete file content:\n\
```\n{}\n```",
        error_message, 
        file_path,
        file_content.lines().count(),
        file_content
    );
    
    let (_thinking_budget, temperature) = analyze_message_complexity(&user_message);
    
    let mut messages = context_messages;
    messages.push(json!({
        "role": "user",
        "content": user_message
    }));

    // Forced tool choice for guaranteed artifacts (disables thinking)
    Ok(json!({
        "model": CONFIG.anthropic_model,
        "max_tokens": CONFIG.anthropic_max_tokens,
        "temperature": temperature,
        "system": system_prompt,
        "messages": messages,
        // NO thinking - incompatible with forced tool choice
        "tools": [get_code_fix_tool_schema()],
        "tool_choice": {
            "type": "tool",
            "name": "provide_code_fix"
        }
    }))
}

pub fn extract_code_fix_response(raw_response: &Value) -> Result<CodeFixResponse> {
    let content = raw_response["content"].as_array()
        .ok_or_else(|| anyhow!("Missing content array in Claude response"))?;
    
    // Find tool call
    let tool_block = content.iter()
        .find(|block| {
            block["type"] == "tool_use" && 
            block["name"] == "provide_code_fix"
        })
        .ok_or_else(|| anyhow!("No provide_code_fix tool call"))?;
    
    let tool_input = &tool_block["input"];
    
    // Parse the tool input as CodeFixResponse
    let response: CodeFixResponse = serde_json::from_value(tool_input.clone())
        .map_err(|e| anyhow!("Failed to parse code fix response: {}", e))?;
    
    // Validate the response
    response.validate()
        .map_err(|e| anyhow!("Validation failed: {}", e))?;
    
    Ok(response)
}

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
    
    pub fn into_complete_response(self, metadata: LLMMetadata, raw: Value) -> CompleteResponse {
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
            structured: StructuredLLMResponse {
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
