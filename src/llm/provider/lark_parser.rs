// src/llm/provider/lark_parser.rs
// Parser for Lark grammar plaintext output from GPT-5 custom tools

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use crate::llm::structured::types::{StructuredLLMResponse, MessageAnalysis};
use tracing::debug;

pub fn parse_lark_output(plaintext: &str) -> Result<StructuredLLMResponse> {
    // Debug logging - you'll see exactly what GPT-5 outputs
    debug!("Parsing Lark output:\n{}", plaintext);
    
    let mut fields: HashMap<String, String> = HashMap::new();
    
    // Parse line by line - extract key:value pairs
    for line in plaintext.lines() {
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(
                key.trim().to_uppercase(),
                value.trim().to_string()
            );
        }
    }
    
    // Extract OUTPUT (the main response text)
    let output = fields
        .get("OUTPUT")
        .ok_or_else(|| anyhow!("Missing OUTPUT field in Lark response"))?
        .clone();
    
    // Parse salience (0.0 - 1.0)
    let salience = fields
        .get("SALIENCE")
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or_else(|| anyhow!("Invalid or missing SALIENCE"))?;
    
    // Parse topics (comma-separated list)
    let topics = fields
        .get("TOPICS")
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_else(|| vec!["general".to_string()]);
    
    // Parse boolean flags
    let contains_code = fields
        .get("CONTAINS_CODE")
        .map(|s| s == "true")
        .unwrap_or(false);
    
    let contains_error = fields
        .get("CONTAINS_ERROR")
        .map(|s| s == "true")
        .unwrap_or(false);
    
    // Parse optional strings (handle "null" literal from grammar)
    let programming_lang = fields
        .get("PROGRAMMING_LANG")
        .filter(|s| *s != "null" && !s.is_empty())
        .cloned();
    
    let error_type = fields
        .get("ERROR_TYPE")
        .filter(|s| *s != "null" && !s.is_empty())
        .cloned();
    
    // Parse routed_to_heads (comma-separated, defaults to semantic)
    let routed_to_heads = fields
        .get("ROUTED_TO_HEADS")
        .map(|s| {
            s.split(',')
                .map(|h| h.trim().to_string())
                .filter(|h| !h.is_empty())
                .collect()
        })
        .unwrap_or_else(|| vec!["semantic".to_string()]);
    
    // Parse language
    let language = fields
        .get("LANGUAGE")
        .ok_or_else(|| anyhow!("Missing LANGUAGE field"))?
        .clone();
    
    // Build the analysis struct
    let analysis = MessageAnalysis {
        salience,
        topics,
        contains_code,
        programming_lang,
        contains_error,
        error_type,
        routed_to_heads,
        language,
        mood: None,
        intensity: None,
        intent: None,
        summary: None,
        relationship_impact: None,
        error_severity: None,
        error_file: None,
    };
    
    Ok(StructuredLLMResponse { 
        output, 
        analysis,
        reasoning: None,
        schema_name: Some("gpt5_lark".to_string()),
        validation_status: Some("valid".to_string()),
    })
}
