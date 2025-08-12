// src/llm/memory_eval.rs
//! Memory evaluation via GPT‑5 + Responses API tool calling.
//! Forces a single `evaluate_memory` tool call and parses arguments into
//! `EvaluateMemoryResponse`.

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::llm::client::OpenAIClient;
use crate::llm::schema::{EvaluateMemoryRequest, EvaluateMemoryResponse};

impl OpenAIClient {
    /// Call GPT‑5 via **/v1/responses** and force a tool call to `evaluate_memory`.
    pub async fn evaluate_memory(&self, req: EvaluateMemoryRequest) -> Result<EvaluateMemoryResponse> {
        // `req.function_schema` should already be a JSON object with:
        // { name, description?, parameters (JSON Schema) }
        let tool = json!({
            "type": "function",
            "function": req.function_schema
        });

        // Force exactly this tool to run
        let tool_choice = json!({
            "type": "function",
            "function": { "name": "evaluate_memory" }
        });

        let body = json!({
            "model": "gpt-5",
            "input": [{
                "role": "user",
                "content": [{ "type": "input_text", "text": req.content }]
            }],
            "tools": [ tool ],
            "tool_choice": tool_choice,
            // We expect the tool args as structured JSON; the tool call itself is the contract.
            "parameters": {
                "verbosity": "low",
                "reasoning_effort": "minimal",
                "max_output_tokens": 256
            }
        });

        let v = self.post_response(body).await?;

        // ----- Preferred: Responses API unified `output[]` with a function_call item -----
        if let Some(arr) = v.get("output").and_then(|o| o.as_array()) {
            for item in arr {
                let is_fn = item.get("type").and_then(|t| t.as_str()) == Some("function_call");
                if !is_fn { continue; }

                // Try several shapes the platform uses in practice
                // 1) { type: "function_call", "function": { "name": "...", "arguments": "..." } }
                if let Some(name) = item.pointer("/function/name").and_then(|x| x.as_str()) {
                    if name == "evaluate_memory" {
                        if let Some(args) = item.pointer("/function/arguments").and_then(|x| x.as_str()) {
                            let parsed: EvaluateMemoryResponse =
                                serde_json::from_str(args).context("parse function.arguments JSON")?;
                            return Ok(parsed);
                        }
                    }
                }
                // 2) { type: "function_call", "name": "...", "arguments": "..." }
                if let Some(name) = item.get("name").and_then(|x| x.as_str()) {
                    if name == "evaluate_memory" {
                        if let Some(args) = item.get("arguments").and_then(|x| x.as_str()) {
                            let parsed: EvaluateMemoryResponse =
                                serde_json::from_str(args).context("parse arguments JSON")?;
                            return Ok(parsed);
                        }
                    }
                }
            }
        }

        // ----- Compat: chat-style tool_calls path -----
        if let Some(args) = v
            .pointer("/choices/0/message/tool_calls/0/function/arguments")
            .and_then(|x| x.as_str())
        {
            let parsed: EvaluateMemoryResponse =
                serde_json::from_str(args).context("parse tool_calls[0].function.arguments JSON")?;
            return Ok(parsed);
        }

        // ----- Last resort: model emitted JSON into output text -----
        if let Some(txt) = v.pointer("/output/0/text").and_then(|x| x.as_str()) {
            let parsed: EvaluateMemoryResponse =
                serde_json::from_str(txt).context("parse JSON from output[0].text")?;
            return Ok(parsed);
        }

        Err(anyhow::anyhow!(
            "evaluate_memory: no function-call arguments found in Responses output"
        ))
    }
}
