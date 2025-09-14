// src/tools/mira_import/openai.rs
// Batch OpenAI memory_eval runner for import using GPT-5 Functions API

use super::schema::MiraMessage;
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use crate::config::CONFIG;

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
    _api_key: &str,  // Unused - kept for backward compatibility
) -> Result<HashMap<String, MemoryEvalResult>> {
    batch_evaluate_messages(messages).await
}

/// Batch evaluate messages using GPT-5 Functions API for memory metadata
pub async fn batch_evaluate_messages(
    messages: &[MiraMessage],
) -> Result<HashMap<String, MemoryEvalResult>> {
    let api_key = std::env::var("OPENAI_API_KEY")?;
    let client = Client::new();
    let mut results = HashMap::new();

    for msg in messages {
        // Skip system messages
        if msg.role == "system" {
            continue;
        }

        // Build the Functions API request for memory evaluation
        let request_body = serde_json::json!({
            "model": CONFIG.gpt5_model,
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
            "function_call": {"name": "memory_eval"},
            "parameters": {
                "verbosity": CONFIG.verbosity,
                "reasoning_effort": CONFIG.reasoning_effort,
                "max_output_tokens": 256
            }
        });

        // Make the API call with retry logic
        let response_json = retry_api_call(&client, &api_key, &request_body).await?;

        // Parse the function call result
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

        // Rate limiting delay
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(results)
}

/// Retry API call with exponential backoff
async fn retry_api_call(
    client: &Client,
    api_key: &str,
    request_body: &serde_json::Value,
) -> Result<serde_json::Value> {
    let max_retries = CONFIG.api_max_retries;
    let mut retry_count = 0;
    let mut retry_delay = Duration::from_millis(CONFIG.api_retry_delay_ms);
    
    loop {
        let response = client
            .post(format!("{}/openai/v1/responses", CONFIG.openai_base_url))  // Fixed endpoint
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;
        
        if response.status().is_success() {
            return Ok(response.json().await?);
        }
        
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        
        // Check if error is retryable
        let is_retryable = status == 429 || status.is_server_error();
        
        if is_retryable && retry_count < max_retries {
            retry_count += 1;
            eprintln!(
                "API request failed (attempt {}/{}), retrying in {:?}: {} - {}",
                retry_count, max_retries, retry_delay, status, error_text
            );
            
            tokio::time::sleep(retry_delay).await;
            
            // Exponential backoff with cap
            retry_delay = Duration::from_millis(
                (retry_delay.as_millis() as u64 * 2).min(10000)
            );
        } else {
            return Err(anyhow::anyhow!(
                "OpenAI API error after {} attempts: {} - {}",
                retry_count + 1, status, error_text
            ));
        }
    }
}

/// Parse function response from GPT-5 response formats
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

    // Try tool_calls format (alternative format)
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
        embedding: vec![],
    })
}

/// Get embedding for text using text-embedding-3-large with retry
async fn get_embedding(client: &Client, api_key: &str, text: &str) -> Result<Vec<f32>> {
    let request_body = serde_json::json!({
        "model": "text-embedding-3-large",
        "input": text,
        "dimensions": CONFIG.qdrant_embedding_dim  // Use config dimension
    });

    let max_retries = CONFIG.api_max_retries;
    let mut retry_count = 0;
    let mut retry_delay = Duration::from_millis(CONFIG.api_retry_delay_ms);
    
    loop {
        let response = client
            .post(format!("{}/v1/embeddings", CONFIG.openai_base_url))  // Fixed endpoint
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if response.status().is_success() {
            let response_json: serde_json::Value = response.json().await?;
            
            let embedding = response_json
                .pointer("/data/0/embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| anyhow::anyhow!("No embedding in response"))?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect::<Vec<_>>();

            return Ok(embedding);
        }
        
        let status = response.status();
        let is_retryable = status == 429 || status.is_server_error();
        
        if is_retryable && retry_count < max_retries {
            retry_count += 1;
            tokio::time::sleep(retry_delay).await;
            retry_delay = Duration::from_millis((retry_delay.as_millis() as u64 * 2).min(10000));
        } else {
            return Err(anyhow::anyhow!("Failed to get embedding after {} attempts", retry_count + 1));
        }
    }
}
