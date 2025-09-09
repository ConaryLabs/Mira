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
        // FIX: Move parameters to top-level, not nested under "parameters"
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
            // These are now top-level fields, not under "parameters"
            "verbosity": "low",
            "reasoning_effort": "minimal",
            "max_output_tokens": 256,
            "temperature": 0.3
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

        // 3. Try the direct output format (simplest)
        if let Some(output) = v.get("output").and_then(|o| o.as_str()) {
            // The output might be the raw arguments JSON
            if let Ok(parsed) = serde_json::from_str::<EvaluateMemoryResponse>(output) {
                return Ok(parsed);
            }
        }

        // If we couldn't parse any format, return an error with debug info
        Err(anyhow!(
            "Could not parse memory evaluation response. Raw response: {:?}",
            v
        ))
    }
}
