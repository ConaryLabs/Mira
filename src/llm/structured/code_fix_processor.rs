// src/llm/structured/code_fix_processor.rs
// Code fix request/response handling using Claude with tool calling

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
    // Rust compiler errors: error[E0308]: ...
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
    if let Some(captures) = regex::Regex::new(r"([^\s:]+\.(?:ts|tsx|js|jsx))\(\d+,\d+\):\s+error")
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
    
    // Generic: Check for common error patterns with file references
    if message.contains("error") || message.contains("Error") {
        // Look for file paths
        if let Some(captures) = regex::Regex::new(r"([a-zA-Z0-9_\-./]+\.[a-z]+):?\d*")
            .ok()?
            .captures(message)
        {
            let file_path = captures.get(1)?.as_str().to_string();
            // Only if it looks like a source file
            if file_path.ends_with(".rs") 
                || file_path.ends_with(".ts") 
                || file_path.ends_with(".tsx")
                || file_path.ends_with(".js")
                || file_path.ends_with(".jsx")
                || file_path.ends_with(".py")
                || file_path.ends_with(".go")
                || file_path.ends_with(".java")
            {
                return Some(ErrorContext {
                    error_message: message.to_string(),
                    file_path,
                    original_line_count: 0,
                });
            }
        }
    }
    
    None
}

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

    Ok(json!({
        "model": CONFIG.anthropic_model,
        "max_tokens": CONFIG.anthropic_max_tokens,
        "temperature": temperature,
        "system": system_prompt,
        "messages": messages,
        "thinking": {
            "type": "enabled",
            "budget_tokens": CONFIG.thinking_budget_complex
        },
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
