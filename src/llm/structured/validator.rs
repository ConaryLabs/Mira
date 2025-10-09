// src/llm/structured/validator.rs
use anyhow::{anyhow, Result};
use super::types::StructuredLLMResponse;

pub fn validate_response(response: &StructuredLLMResponse) -> Result<()> {
    if response.output.trim().is_empty() {
        return Err(anyhow!("output cannot be empty (memory_entries.content NOT NULL)"));
    }
    
    // Salience should be 0.0-1.0
    if response.analysis.salience < 0.0 || response.analysis.salience > 1.0 {
        return Err(anyhow!(
            "salience {} violates CHECK(salience >= 0 AND salience <= 1)",
            response.analysis.salience
        ));
    }
    
    // Validate intensity if present
    if let Some(intensity) = response.analysis.intensity {
        if intensity < 0.0 || intensity > 1.0 {
            return Err(anyhow!(
                "intensity {} must be between 0.0 and 1.0",
                intensity
            ));
        }
    }
    
    if response.analysis.topics.is_empty() {
        return Err(anyhow!("topics cannot be empty (required for tags field)"));
    }
    
    if response.analysis.routed_to_heads.is_empty() {
        return Err(anyhow!("routed_to_heads cannot be empty (at least one head required)"));
    }
    
    const VALID_HEADS: &[&str] = &["semantic", "code", "summary", "documents"];
    for head in &response.analysis.routed_to_heads {
        if !VALID_HEADS.contains(&head.as_str()) {
            return Err(anyhow!("invalid memory head '{}', must be one of: {:?}", 
                              head, VALID_HEADS));
        }
    }
    
    if response.analysis.contains_code && response.analysis.programming_lang.is_none() {
        return Err(anyhow!("programming_lang is required when contains_code=true"));
    }
    
    // Validate error fields consistency
    if response.analysis.contains_error {
        if response.analysis.error_type.is_none() {
            return Err(anyhow!("error_type is required when contains_error=true"));
        }
    }
    
    Ok(())
}
