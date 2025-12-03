// src/llm/provider/gemini3/response.rs
// Response parsing helpers for Gemini 3

use anyhow::{anyhow, Result};
use serde_json::Value;
use tracing::info;

use crate::llm::provider::TokenUsage;

/// Extract the first candidate from a Gemini response
pub fn extract_first_candidate(response: &Value) -> Result<&Value> {
    response
        .get("candidates")
        .and_then(|c| c.get(0))
        .ok_or_else(|| anyhow!("No candidates in Gemini response"))
}

/// Extract parts array from a candidate
pub fn extract_parts(candidate: &Value) -> Option<&Vec<Value>> {
    candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
}

/// Extract text content from the first part of a candidate
pub fn extract_text_content(candidate: &Value) -> String {
    candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string()
}

/// Extract token usage from Gemini response usageMetadata
pub fn extract_token_usage(response: &Value) -> TokenUsage {
    let usage = response.get("usageMetadata");
    TokenUsage {
        input: usage
            .and_then(|u| u.get("promptTokenCount"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0),
        output: usage
            .and_then(|u| u.get("candidatesTokenCount"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0),
        reasoning: usage
            .and_then(|u| u.get("thoughtsTokenCount"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0),
        cached: usage
            .and_then(|u| u.get("cachedContentTokenCount"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0),
    }
}


/// Log token usage with cache information
pub fn log_token_usage(context: &str, tokens: &TokenUsage) {
    if tokens.cached > 0 && tokens.input > 0 {
        let cache_percent = (tokens.cached as f64 / tokens.input as f64 * 100.0) as i64;
        info!(
            "{}: {} input ({} cached = {}% savings), {} output, {} thinking",
            context, tokens.input, tokens.cached, cache_percent, tokens.output, tokens.reasoning
        );
    } else {
        info!(
            "{}: {} input tokens, {} output tokens, {} thinking tokens (no cache hit)",
            context, tokens.input, tokens.output, tokens.reasoning
        );
    }
}

/// Log token usage for tool calls (simpler format, no reasoning)
pub fn log_tool_call_tokens(context: &str, input: i64, output: i64, cached: i64) {
    if cached > 0 && input > 0 {
        let cache_percent = (cached as f64 / input as f64 * 100.0) as i64;
        info!(
            "{}: {} input ({} cached = {}% savings), {} output",
            context, input, cached, cache_percent, output
        );
    }
}

/// Log code generation token usage
pub fn log_codegen_tokens(context: &str, lines: usize, path: &str, input: i64, cached: i64) {
    if cached > 0 && input > 0 {
        let cache_percent = (cached as f64 / input as f64 * 100.0) as i64;
        info!(
            "{}: Generated {} lines at {} ({} cached = {}% savings)",
            context, lines, path, cached, cache_percent
        );
    } else {
        info!(
            "{}: Generated {} lines of code at {}",
            context, lines, path
        );
    }
}
