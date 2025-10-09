// src/llm/structured/code_fix_processor.rs
// Code fix request/response handling with unified provider support
// Works with both DeepSeek 3.2 and GPT-5 Responses API

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::warn;

use crate::config::CONFIG;
use super::types::{CompleteResponse, LLMMetadata, StructuredLLMResponse, MessageAnalysis};
use super::tool_schema::get_code_fix_tool_schema;
use super::analyze_message_complexity;
use crate::memory::features::message_pipeline::analyzers::ChatAnalysisResult;

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
    pub error_type: String,
    pub error_severity: String,
    pub original_line_count: usize,
}

/// Extract error context from LLM analysis result
pub fn extract_error_context(analysis: &ChatAnalysisResult) -> Option<ErrorContext> {
    if !analysis.contains_error.unwrap_or(false) {
        return None;
    }
    
    let error_type = analysis.error_type.as_ref()?;
    
    Some(ErrorContext {
        error_message: analysis.content.clone(),
        file_path: analysis.error_file.clone().unwrap_or_else(|| "unknown".to_string()),
        error_type: error_type.clone(),
        error_severity: analysis.error_severity.clone().unwrap_or_else(|| "warning".to_string()),
        original_line_count: 0,
    })
}

pub fn build_code_fix_request(
    error_message: &str,
    file_path: &str,
    file_content: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
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
        "model": CONFIG.deepseek_model,
        "max_tokens": CONFIG.deepseek_max_tokens,
        "temperature": temperature,
        "system": system_prompt,
        "messages": messages,
        "tools": [get_code_fix_tool_schema()],
        "tool_choice": {
            "type": "tool",
            "name": "provide_code_fix"
        }
    }))
}

/// Extract code fix from ToolResponse raw_response
/// OpenAI/DeepSeek/GPT-5 format only
pub fn extract_code_fix_response(raw_response: &Value) -> Result<CodeFixResponse> {
    // OpenAI/GPT-5/DeepSeek format: choices[0].message.tool_calls
    let choices = raw_response["choices"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing choices array in response"))?;
    
    let message = choices
        .get(0)
        .and_then(|c| c.get("message"))
        .ok_or_else(|| anyhow!("Missing message in choices[0]"))?;
    
    let tool_calls = message["tool_calls"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing tool_calls array in message"))?;
    
    let tool_call = tool_calls
        .iter()
        .find(|call| call["function"]["name"] == "provide_code_fix")
        .ok_or_else(|| anyhow!("No provide_code_fix tool call found"))?;
    
    // Parse arguments (they're a JSON string in OpenAI format)
    let args_str = tool_call["function"]["arguments"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing function arguments"))?;
    
    let tool_input: Value = serde_json::from_str(args_str)
        .map_err(|e| anyhow!("Failed to parse arguments JSON: {}", e))?;
    
    let response: CodeFixResponse = serde_json::from_value(tool_input)
        .map_err(|e| anyhow!("Failed to parse code fix response: {}", e))?;
    
    response.validate()
        .map_err(|e| anyhow!("Validation failed: {}", e))?;
    
    Ok(response)
}

impl CodeFixResponse {
    pub fn validate(&self) -> Result<(), String> {
        for file in &self.files {
            if file.content.contains("...") {
                return Err(format!("File {} contains ellipsis", file.path));
            }
            if file.content.contains("// rest") || file.content.contains("// unchanged") {
                return Err(format!("File {} contains snippets", file.path));
            }
        }
        
        if self.confidence < 0.5 {
            warn!("Low confidence fix: {}", self.confidence);
        }
        
        let has_primary = self.files.iter().any(|f| matches!(f.change_type, ChangeType::Primary));
        if !has_primary {
            return Err("No primary file marked in fix".to_string());
        }
        
        Ok(())
    }
    
    pub fn validate_line_counts(&self, error_context: &ErrorContext) -> Vec<String> {
        let mut warnings = Vec::new();
        
        for file in &self.files {
            if matches!(file.change_type, ChangeType::Primary) {
                let fixed_lines = file.content.lines().count();
                let original_lines = error_context.original_line_count;
                
                if original_lines > 0 && fixed_lines < (original_lines / 2) {
                    warnings.push(format!(
                        "Fixed file {} has {} lines vs original {} lines (less than 50%)",
                        file.path, fixed_lines, original_lines
                    ));
                }
                
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
