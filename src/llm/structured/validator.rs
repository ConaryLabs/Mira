// src/llm/structured/validator.rs

use anyhow::{anyhow, Result};
use super::types::StructuredGPT5Response;

pub fn validate_response(response: &StructuredGPT5Response) -> Result<()> {
    if response.output.trim().is_empty() {
        return Err(anyhow!("output cannot be empty (memory_entries.content NOT NULL)"));
    }
    
    if response.analysis.salience < 0.0 || response.analysis.salience > 10.0 {
        return Err(anyhow!(
            "salience {} violates CHECK(salience >= 0 AND salience <= 10)",
            response.analysis.salience
        ));
    }
    
    if let Some(intensity) = response.analysis.intensity {
        if intensity < 0.0 || intensity > 1.0 {
            return Err(anyhow!(
                "intensity {} violates CHECK(intensity >= 0 AND intensity <= 1)",
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
    
    if let Some(ref lang) = response.analysis.programming_lang {
        const VALID_LANGS: &[&str] = &["rust", "typescript", "javascript", "python", "go", "java"];
        if !VALID_LANGS.contains(&lang.as_str()) {
            return Err(anyhow!("programming_lang '{}' not in language_configs", lang));
        }
    }
    
    Ok(())
}
