// src/llm/memory_eval.rs

use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

use crate::llm::client::OpenAIClient;
use crate::llm::schema::{EvaluateMemoryRequest, EvaluateMemoryResponse};

impl OpenAIClient {
    pub async fn evaluate_memory(&self, req: EvaluateMemoryRequest) -> Result<EvaluateMemoryResponse> {
        let max_retries = 3;
        let mut attempt = 0;
        
        loop {
            attempt += 1;
            match self.evaluate_memory_attempt(&req).await {
                Ok(response) => return Ok(response),
                Err(e) if attempt < max_retries => {
                    let error_str = e.to_string();
                    if error_str.contains("429") || error_str.contains("5") {
                        let jitter = Duration::from_millis(100 * attempt as u64 + rand::random::<u64>() % 100);
                        eprintln!("Memory evaluation attempt {} failed ({}), retrying after {:?}...", 
                                 attempt, error_str, jitter);
                        sleep(jitter).await;
                        continue;
                    }
                    return Err(e);
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn evaluate_memory_attempt(&self, req: &EvaluateMemoryRequest) -> Result<EvaluateMemoryResponse> {
        // Convert function schema to JSON schema for JSON mode
        let json_schema = if let Some(params) = req.function_schema.get("parameters") {
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "memory_evaluation",
                    "strict": true,
                    "schema": params
                }
            })
        } else {
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "memory_evaluation",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "salience": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 10
                            },
                            "tags": {
                                "type": "array",
                                "items": {"type": "string"}
                            },
                            "memory_type": {
                                "type": "string",
                                "enum": ["feeling", "fact", "joke", "promise", "event", "other"]
                            },
                            "summary": {
                                "type": "string"
                            }
                        },
                        "required": ["salience", "tags", "memory_type"]
                    }
                }
            })
        };

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
            "instructions": "Analyze the message and return a JSON object with memory evaluation metadata.",
            "max_output_tokens": 256,
            "temperature": 0.3,
            "text": {
                "verbosity": "low",
                "format": json_schema
            },
            "reasoning": {
                "effort": "minimal"
            }
        });

        let v = self.post_response(body).await
            .context("Failed to call GPT-5 for memory evaluation")?;

        // Extract text content from response
        let content = if let Some(text) = v.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
            text
        } else if let Some(text) = v.pointer("/output/message/content/0/text/value").and_then(|t| t.as_str()) {
            text
        } else if let Some(text) = v.pointer("/output/message/content/0/text").and_then(|t| t.as_str()) {
            text
        } else if let Some(text) = v.get("output").and_then(|o| o.as_str()) {
            text
        } else {
            return Err(anyhow!(
                "Could not extract text from response. Raw response: {:?}",
                v
            ));
        };

        // Parse the JSON response
        serde_json::from_str::<EvaluateMemoryResponse>(content)
            .context("Failed to parse memory evaluation JSON response")
    }
}
