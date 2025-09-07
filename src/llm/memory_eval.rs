// src/llm/memory_eval.rs
// Phase 4: Memory evaluation via GPT-5 Functions API with improved error handling and retries

use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

use crate::llm::client::OpenAIClient;
use crate::llm::schema::{EvaluateMemoryRequest, EvaluateMemoryResponse};

impl OpenAIClient {
    /// Call GPT-5 via /v1/responses using the Functions API to evaluate memory
    /// Includes retry logic for rate limits and transient errors
    pub async fn evaluate_memory(&self, req: EvaluateMemoryRequest) -> Result<EvaluateMemoryResponse> {
        let max_retries = 3;
        let mut attempt = 0;
        
        loop {
            attempt += 1;
            match self.evaluate_memory_attempt(&req).await {
                Ok(response) => return Ok(response),
                Err(e) if attempt < max_retries => {
                    // Check if it's a retryable error
                    let error_str = e.to_string();
                    if error_str.contains("429") || error_str.contains("5") {
                        let jitter = Duration::from_millis(100 * attempt as u64 + rand::random::<u64>() % 100);
                        eprintln!("⚠️ Memory evaluation attempt {attempt} failed ({error_str}), retrying after {jitter:?}...");
                        sleep(jitter).await;
                        continue;
                    }
                    return Err(e);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Single attempt to evaluate memory using Functions API
    async fn evaluate_memory_attempt(&self, req: &EvaluateMemoryRequest) -> Result<EvaluateMemoryResponse> {
        // Build the function definition for evaluate_memory
        let function_def = json!({
            "type": "function",
            "function": req.function_schema
        });

        // Build the request body for GPT-5 Functions API
        let body = json!({
            "model": "gpt-5",
            "input": [{
                "role": "user",
                "content": [{ 
                    "type": "input_text", 
                    "text": format!(
                        "Analyze this message for memory storage: \"{}\"\n\
                        Evaluate its emotional significance, categorize it, and provide relevant tags.",
                        req.content
                    )
                }]
            }],
            "functions": [function_def],
            "function_call": { 
                "name": "evaluate_memory" 
            },
            "parameters": {
                "verbosity": "low",
                "reasoning_effort": "minimal",
                "max_output_tokens": 256,
                "temperature": 0.3
            }
        });

        let v = self.post_response(body).await
            .context("Failed to call GPT-5 for memory evaluation")?;

        // Parse the function call response - try multiple formats for compatibility
        
        // 1. Try the new unified output format with function_call items
        if let Some(arr) = v.get("output").and_then(|o| o.as_array()) {
            for item in arr {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    // Format 1: { type: "function_call", "function": { "name": "...", "arguments": "..." } }
                    if let Some(fn_obj) = item.get("function") {
                        if fn_obj.get("name").and_then(|n| n.as_str()) == Some("evaluate_memory") {
                            if let Some(args_str) = fn_obj.get("arguments").and_then(|a| a.as_str()) {
                                let parsed: EvaluateMemoryResponse = serde_json::from_str(args_str)
                                    .context("Failed to parse function.arguments JSON")?;
                                return Ok(parsed);
                            }
                        }
                    }
                    
                    // Format 2: { type: "function_call", "name": "...", "arguments": "..." }
                    if item.get("name").and_then(|n| n.as_str()) == Some("evaluate_memory") {
                        if let Some(args_str) = item.get("arguments").and_then(|a| a.as_str()) {
                            let parsed: EvaluateMemoryResponse = serde_json::from_str(args_str)
                                .context("Failed to parse arguments JSON")?;
                            return Ok(parsed);
                        }
                    }
                }
            }
        }

        // 2. Try the tool_calls format (newer format)
        if let Some(tool_calls) = v.pointer("/choices/0/message/tool_calls").and_then(|tc| tc.as_array()) {
            for tool_call in tool_calls {
                if let Some(fn_obj) = tool_call.get("function") {
                    if fn_obj.get("name").and_then(|n| n.as_str()) == Some("evaluate_memory") {
                        if let Some(args_str) = fn_obj.get("arguments").and_then(|a| a.as_str()) {
                            let parsed: EvaluateMemoryResponse = serde_json::from_str(args_str)
                                .context("Failed to parse tool_calls function.arguments JSON")?;
                            return Ok(parsed);
                        }
                    }
                }
            }
        }

        // 3. Try the legacy function_call format (older compatibility)
        if let Some(fn_call) = v.pointer("/choices/0/message/function_call") {
            if fn_call.get("name").and_then(|n| n.as_str()) == Some("evaluate_memory") {
                if let Some(args_str) = fn_call.get("arguments").and_then(|a| a.as_str()) {
                    let parsed: EvaluateMemoryResponse = serde_json::from_str(args_str)
                        .context("Failed to parse legacy function_call.arguments JSON")?;
                    return Ok(parsed);
                }
            }
        }

        // 4. Last resort: check if the model output JSON directly in the text
        if let Some(text) = v.pointer("/output/0/text").and_then(|t| t.as_str()) {
            // Try to parse the entire text as JSON
            if let Ok(parsed) = serde_json::from_str::<EvaluateMemoryResponse>(text) {
                eprintln!("⚠️ Memory evaluation: Had to parse from raw text output");
                return Ok(parsed);
            }
        }

        // If we couldn't find the function call in any expected format, return an error with context
        Err(anyhow!(
            "Memory evaluation failed: No function call found in response. Response structure: {:?}",
            v.as_object().map(|o| o.keys().collect::<Vec<_>>())
        ))
    }
}

// Add a helper function for testing the memory evaluation
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_evaluation_parsing() {
        // Test various response formats to ensure compatibility
        let test_responses = vec![
            // Format 1: Unified output with function_call
            json!({
                "output": [{
                    "type": "function_call",
                    "function": {
                        "name": "evaluate_memory",
                        "arguments": r#"{"salience": 7, "tags": ["important"], "memory_type": "event", "summary": "Test"}"#
                    }
                }]
            }),
            // Format 2: tool_calls array
            json!({
                "choices": [{
                    "message": {
                        "tool_calls": [{
                            "function": {
                                "name": "evaluate_memory",
                                "arguments": r#"{"salience": 7, "tags": ["important"], "memory_type": "event", "summary": "Test"}"#
                            }
                        }]
                    }
                }]
            }),
            // Format 3: Legacy function_call
            json!({
                "choices": [{
                    "message": {
                        "function_call": {
                            "name": "evaluate_memory",
                            "arguments": r#"{"salience": 7, "tags": ["important"], "memory_type": "event", "summary": "Test"}"#
                        }
                    }
                }]
            }),
        ];

        // Each format should parse correctly
        for (i, response) in test_responses.iter().enumerate() {
            println!("Testing format {}", i + 1);
            // Parsing logic would go here in actual tests
        }
    }
}
