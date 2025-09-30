// src/llm/structured/code_fix_processor.rs
// Code fix request/response handling using Claude

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::warn;

use super::types::{CompleteResponse, LLMMetadata, StructuredLLMResponse, MessageAnalysis};

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

pub fn build_code_fix_request(
    error_message: &str,
    file_path: &str,
    file_content: &str,
    system_prompt: String,
    context_messages: Vec<Value>,
) -> Result<Value> {
    // Build comprehensive user message with JSON schema
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
```\n{}\n```\n\
        \n\
        Respond with JSON matching this EXACT schema:\n\
        {{\n\
          \"output\": \"explanation of the fix\",\n\
          \"analysis\": {{\n\
            \"salience\": 8.0,\n\
            \"topics\": [\"error_fix\", \"compiler_error\"],\n\
            \"contains_code\": true,\n\
            \"routed_to_heads\": [\"code\"],\n\
            \"language\": \"en\",\n\
            \"mood\": null,\n\
            \"intensity\": null,\n\
            \"intent\": \"fix_code\",\n\
            \"summary\": \"Fixed compiler error\",\n\
            \"relationship_impact\": null,\n\
            \"programming_lang\": \"rust\"\n\
          }},\n\
          \"reasoning\": \"your detailed reasoning\",\n\
          \"fix_type\": \"compiler_error\",\n\
          \"files\": [\n\
            {{\n\
              \"path\": \"{}\",\n\
              \"content\": \"COMPLETE FILE CONTENT HERE\",\n\
              \"change_type\": \"primary\"\n\
            }}\n\
          ],\n\
          \"confidence\": 0.95\n\
        }}",
        error_message, file_path, 
        file_content.lines().count(),
        file_content,
        file_path
    );
    
    // Use Claude request builder
    super::claude_processor::build_claude_request(
        &user_message,
        system_prompt,
        context_messages,
    )
}

pub fn extract_code_fix_response(raw_response: &Value) -> Result<CodeFixResponse> {
    let content = raw_response["content"].as_array()
        .ok_or_else(|| anyhow!("Missing content array in Claude response"))?;
    
    // Find text content block
    let text_content = content.iter()
        .find(|block| block["type"] == "text")
        .and_then(|block| block["text"].as_str())
        .ok_or_else(|| anyhow!("No text content in Claude response"))?;
    
    // Parse JSON from text content
    let response: CodeFixResponse = serde_json::from_str(text_content)
        .map_err(|e| anyhow!("Failed to parse code fix response as JSON: {}", e))?;
    
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
