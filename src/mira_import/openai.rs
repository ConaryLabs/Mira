// src/mira_import/openai.rs

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

pub async fn batch_memory_eval(
    messages: &[MiraMessage],
    _api_key: &str,
) -> Result<HashMap<String, MemoryEvalResult>> {
    batch_evaluate_messages(messages).await
}

pub async fn batch_evaluate_messages(
    messages: &[MiraMessage],
) -> Result<HashMap<String, MemoryEvalResult>> {
    let api_key = std::env::var("OPENAI_API_KEY")?;
    let client = Client::new();
    let mut results = HashMap::new();

    for msg in messages {
        if msg.role == "system" {
            continue;
        }

        // Build request using Responses API with JSON mode (Sept 2025)
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
            "instructions": "Analyze the message and return a JSON object with: salience (1-10 emotional importance), tags (array of contextual tags), and memory_type (feeling/fact/joke/promise/event/other).",
            "max_output_tokens": 256,
            "text": {
                "verbosity": CONFIG.verbosity,
                "format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "memory_eval",
                        "strict": true,
                        "schema": {
                            "type": "object",
                            "properties": {
                                "salience": {
                                    "type": "number",
                                    "minimum": 1,
                                    "maximum": 10
                                },
                                "tags": {
                                    "type": "array",
                                    "items": {"type": "string"}
                                },
                                "memory_type": {
                                    "type": "string",
                                    "enum": ["feeling", "fact", "joke", "promise", "event", "other"]
                                }
                            },
                            "required": ["salience", "tags", "memory_type"]
                        }
                    }
                }
            },
            "reasoning": {
                "effort": CONFIG.reasoning_effort
            }
        });

        let response_json = retry_api_call(&client, &api_key, &request_body).await?;

        let eval_result = parse_json_response(&response_json)?;

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

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(results)
}

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
            .post(format!("{}/v1/responses", CONFIG.openai_base_url))
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
        
        let is_retryable = status == 429 || status.is_server_error();
        
        if is_retryable && retry_count < max_retries {
            retry_count += 1;
            eprintln!(
                "API request failed (attempt {}/{}), retrying in {:?}: {} - {}",
                retry_count, max_retries, retry_delay, status, error_text
            );
            
            tokio::time::sleep(retry_delay).await;
            
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

fn parse_json_response(response: &serde_json::Value) -> Result<MemoryEvalResult> {
    // Extract text content using the same paths as main client
    let content = if let Some(text) = response.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
        text
    } else if let Some(text) = response.pointer("/output/message/content/0/text/value").and_then(|t| t.as_str()) {
        text
    } else if let Some(text) = response.pointer("/output/message/content/0/text").and_then(|t| t.as_str()) {
        text
    } else if let Some(text) = response.get("output").and_then(|o| o.as_str()) {
        text
    } else {
        return Ok(MemoryEvalResult {
            salience: 5.0,
            tags: vec!["imported".to_string()],
            memory_type: "other".to_string(),
            embedding: vec![],
        });
    };

    // Parse the JSON content
    let eval_data: serde_json::Value = serde_json::from_str(content)?;
    
    Ok(MemoryEvalResult {
        salience: eval_data.get("salience")
            .and_then(|s| s.as_f64())
            .unwrap_or(5.0) as f32,
        tags: eval_data.get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["imported".to_string()]),
        memory_type: eval_data.get("memory_type")
            .and_then(|m| m.as_str())
            .unwrap_or("other")
            .to_string(),
        embedding: vec![],
    })
}

async fn get_embedding(client: &Client, api_key: &str, text: &str) -> Result<Vec<f32>> {
    let request_body = serde_json::json!({
        "model": "text-embedding-3-large",
        "input": text,
        "dimensions": CONFIG.qdrant_embedding_dim
    });

    let max_retries = CONFIG.api_max_retries;
    let mut retry_count = 0;
    let mut retry_delay = Duration::from_millis(CONFIG.api_retry_delay_ms);
    
    loop {
        let response = client
            .post(format!("{}/v1/embeddings", CONFIG.openai_base_url))
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
