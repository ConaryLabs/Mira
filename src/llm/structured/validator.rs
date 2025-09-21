// src/llm/structured/validator.rs
// Validation for structured responses - enforce ALL database constraints

use anyhow::{anyhow, Result};
use super::types::StructuredGPT5Response;

/// Validate all database constraints before saving anything
/// This is our shield against corrupt data entering the system
pub fn validate_response(response: &StructuredGPT5Response) -> Result<()> {
    // Validate memory_entries constraints
    if response.output.trim().is_empty() {
        return Err(anyhow!("output cannot be empty (memory_entries.content NOT NULL)"));
    }
    
    // Validate message_analysis constraints
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
    
    // Validate memory head values against known heads
    const VALID_HEADS: &[&str] = &["semantic", "code", "summary", "documents"];
    for head in &response.analysis.routed_to_heads {
        if !VALID_HEADS.contains(&head.as_str()) {
            return Err(anyhow!("invalid memory head '{}', must be one of: {:?}", 
                              head, VALID_HEADS));
        }
    }
    
    // Validate code intelligence requirements
    if response.analysis.contains_code && response.analysis.programming_lang.is_none() {
        return Err(anyhow!("programming_lang is required when contains_code=true"));
    }
    
    if let Some(ref lang) = response.analysis.programming_lang {
        const VALID_LANGS: &[&str] = &["rust", "typescript", "javascript", "python", "go", "java"];
        if !VALID_LANGS.contains(&lang.as_str()) {
            return Err(anyhow!("programming_lang '{}' not in supported languages: {:?}", 
                              lang, VALID_LANGS));
        }
    }
    
    // Validate language field
    if response.analysis.language.trim().is_empty() {
        return Err(anyhow!("language cannot be empty"));
    }
    
    // Additional validation for topic quality
    for topic in &response.analysis.topics {
        if topic.trim().is_empty() {
            return Err(anyhow!("topics cannot contain empty strings"));
        }
        if topic.len() > 100 {
            return Err(anyhow!("topic '{}' too long (max 100 chars)", topic));
        }
    }
    
    // Validate summary length if present
    if let Some(ref summary) = response.analysis.summary {
        if summary.len() > 500 {
            return Err(anyhow!("summary too long (max 500 chars)"));
        }
    }
    
    // Validate intent if present
    if let Some(ref intent) = response.analysis.intent {
        if intent.trim().is_empty() {
            return Err(anyhow!("intent cannot be empty string if provided"));
        }
    }
    
    // Validate mood if present
    if let Some(ref mood) = response.analysis.mood {
        if mood.trim().is_empty() {
            return Err(anyhow!("mood cannot be empty string if provided"));
        }
    }
    
    Ok(())
}

/// Additional validation for complete response
pub fn validate_complete_response(response: &super::types::CompleteResponse) -> Result<()> {
    // Validate the structured part
    validate_response(&response.structured)?;
    
    // Validate metadata makes sense
    if response.metadata.latency_ms < 0 {
        return Err(anyhow!("latency_ms cannot be negative"));
    }
    
    if response.metadata.latency_ms > 300000 { // 5 minutes
        return Err(anyhow!("latency_ms {} seems unreasonably high", response.metadata.latency_ms));
    }
    
    // Validate token counts if present
    if let Some(tokens) = response.metadata.total_tokens {
        if tokens < 0 {
            return Err(anyhow!("total_tokens cannot be negative"));
        }
        if tokens > 200000 { // GPT-5 max
            return Err(anyhow!("total_tokens {} exceeds reasonable limit", tokens));
        }
    }
    
    // Ensure prompt_tokens + completion_tokens = total_tokens if all present
    if let (Some(prompt), Some(completion), Some(total)) = (
        response.metadata.prompt_tokens,
        response.metadata.completion_tokens,
        response.metadata.total_tokens
    ) {
        let calculated_total = prompt + completion + response.metadata.reasoning_tokens.unwrap_or(0);
        if calculated_total != total {
            return Err(anyhow!(
                "token count mismatch: prompt({}) + completion({}) + reasoning({}) != total({})",
                prompt, completion, response.metadata.reasoning_tokens.unwrap_or(0), total
            ));
        }
    }
    
    Ok(())
}
