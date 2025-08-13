// src/tools/mira_import/openai.rs
// Phase 9: Updated to use GPT-5 Functions API

//! Batch OpenAI memory_eval runner for import (GPT-5, strict, retcon)

use super::schema::MiraMessage;
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemoryEvalResult {
    pub salience: f32,
    pub tags: Vec<String>,
    pub memory_type: String,
    pub embedding: Vec<f32>,
}

/// Batch evaluate messages using GPT-5 Functions API for memory metadata
/// This is the main export for the import tool
pub async fn batch_memory_eval(
    messages: &[MiraMessage],
    api_key: &str,
) -> Result<HashMap<String, MemoryEvalResult>> {
    batch_evaluate_messages(messages).await
}

/// Batch evaluate messages using GPT-5 Functions API for memory metadata
pub async fn batch_evaluate_messages(
    messages: &[MiraMessage],
) -> Result<HashMap<String, MemoryEvalResult>> {
    let api_key = env::var("OPENAI_API_KEY")?;
    let client = Client::new();
    let mut results = HashMap::new();

    for msg in messages {
        // Skip system messages
        if msg.role == "system" {
            continue;
        }

        // Build the Functions API request for memory evaluation
        let request_body = serde_json::json!({
            "model": "gpt-5",
            "input": [
                {
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": format!("Evaluate this message for memory storage: \"{}\"", msg.content)
                    }]
                }
            ],
            "functions": [
                {
                    "name": "memory_eval",
                    "description": "Evaluate a message for memory storage metadata",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "salience": {
                                "type": "number",
                                "minimum": 1,
                                "maximum": 10,
                                "description": "Emotional importance (1-10)"
                            },
                            "tags": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "Contextual tags for this memory"
                            },
                            "memory_type": {
                                "type": "string",
                                "enum": ["feeling", "fact", "joke", "promise", "event", "other"],
                                "description": "Type of memory"
                            }
                        },
                        "required": ["salience", "tags", "memory_type"]
                    }
                }
            ],
            "function_call": {"name": "memory_eval"},  // Force this function to be called
            "parameters": {
                "verbosity": "low",
                "reasoning_effort": "minimal",
                "max_output_tokens": 256
            }
        });

        // Make the API call to /v1/responses
        let mut retry_count = 0;
        let max_retries = 3;
        
        let response_json = loop {
            let response = client
                .post("https://api.openai.com/v1/responses")
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .send()
                .await?;
            
            if response.status().is_success() {
                break response.json().await?;
            } else if response.status() == 429 || response.status().is_server_error() {
                // Rate limit or server error - retry with backoff
                if retry_count < max_retries {
                    retry_count += 1;
                    let jitter = std::time::Duration::from_millis(100 * retry_count);
                    tokio::time::sleep(jitter).await;
                    continue;
                }
            }
            
            // If we get here, it's a non-retryable error
            return Err(anyhow::anyhow!(
                "OpenAI API error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        };

        // Parse the function call result from various possible formats
        let eval_result = parse_function_response(&response_json)?;

        // Get embedding for the message
        let embedding = get_embedding(&client, &api_key, &msg.content).await?;

        results.insert(
            msg.message_id.clone(),
            MemoryEvalResult {
                salience: eval_result.salience,
                tags: eval_result.tags,
                memory_type: eval_result.memory_type,
                embedding,
            },
        );

        // Small delay to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    Ok(results)
}

/// Parse function response from various GPT-5 response formats
fn parse_function_response(response: &serde_json::Value) -> Result<MemoryEvalResult> {
    // Try unified output format first
    if let Some(output) = response.get("output").and_then(|o| o.as_array()) {
        for item in output {
            if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                if let Some(function) = item.get("function") {
                    if function.get("name").and_then(|n| n.as_str()) == Some("memory_eval") {
                        if let Some(args_str) = function.get("arguments").and_then(|a| a.as_str()) {
                            return parse_eval_args(args_str);
                        }
                    }
                }
            }
        }
    }

    // Try tool_calls format (newer format)
    if let Some(tool_calls) = response
        .pointer("/choices/0/message/tool_calls")
        .and_then(|tc| tc.as_array())
    {
        for tool_call in tool_calls {
            if let Some(function) = tool_call.get("function") {
                if function.get("name").and_then(|n| n.as_str()) == Some("memory_eval") {
                    if let Some(args_str) = function.get("arguments").and_then(|a| a.as_str()) {
                        return parse_eval_args(args_str);
                    }
                }
            }
        }
    }

    // Fallback defaults
    Ok(MemoryEvalResult {
        salience: 5.0,
        tags: vec!["imported".to_string()],
        memory_type: "other".to_string(),
        embedding: vec![],
    })
}

/// Parse evaluation arguments from JSON string
fn parse_eval_args(args_str: &str) -> Result<MemoryEvalResult> {
    let args: serde_json::Value = serde_json::from_str(args_str)?;
    
    Ok(MemoryEvalResult {
        salience: args.get("salience")
            .and_then(|s| s.as_f64())
            .unwrap_or(5.0) as f32,
        tags: args.get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["imported".to_string()]),
        memory_type: args.get("memory_type")
            .and_then(|m| m.as_str())
            .unwrap_or("other")
            .to_string(),
        embedding: vec![], // Will be filled by get_embedding
    })
}

/// Get embedding for text using text-embedding-3-large
async fn get_embedding(client: &Client, api_key: &str, text: &str) -> Result<Vec<f32>> {
    let request_body = serde_json::json!({
        "model": "text-embedding-3-large",
        "input": text,
        "dimensions": 3072
    });

    let response = client
        .post("https://api.openai.com/v1/embeddings")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    let response_json: serde_json::Value = response.json().await?;
    
    let embedding = response_json
        .pointer("/data/0/embedding")
        .and_then(|e| e.as_array())
        .ok_or_else(|| anyhow::anyhow!("No embedding in response"))?
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect::<Vec<_>>();

    Ok(embedding)
}
