// backend/src/tools/mira_import/openai.rs

//! Batch OpenAI memory_eval runner for import (GPT-4.1, strict, retcon)
//! - Prepares batch jobs for all messages
//! - Submits to OpenAI /v1/batch API
//! - Collects results and aligns with original messages
//! - Falls back to live API if batch is unavailable

use super::schema::MiraMessage;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use reqwest::{Client, StatusCode};
use tokio::time::sleep;

/// Result of running memory_eval on a message
#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryEvalResult {
    pub persona: String,
    pub mood: String,
    pub salience: f32,
    pub tags: Vec<String>,
    pub memory_type: String,
    pub embedding: Vec<f32>,
    pub eval_cost: Option<f32>,
    pub embedding_cost: Option<f32>,
}

/// Submit all messages to OpenAI batch endpoint, return results keyed by message_id
pub async fn batch_memory_eval(
    messages: &[MiraMessage],
    api_key: &str,
) -> anyhow::Result<HashMap<String, MemoryEvalResult>> {
    // Build batch job JSON (max 10,000 jobs per batch)
    let jobs: Vec<_> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "custom_id": m.message_id,
                "method": "POST",
                "url": "https://api.openai.com/v1/chat/completions",
                "body": {
                    "model": "gpt-4.1",
                    "messages": [
                        {"role": &m.role, "content": &m.content}
                    ],
                    "tools": [{
                        "type": "function",
                        "function": {
                            "name": "memory_eval",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "persona": {"type": "string"},
                                    "mood": {"type": "string"},
                                    "salience": {"type": "number"},
                                    "tags": {"type": "array", "items": {"type": "string"}},
                                    "memory_type": {"type": "string"}
                                },
                                "required": ["persona", "mood", "salience", "tags", "memory_type"]
                            }
                        }
                    }],
                    "tool_choice": {"type": "function", "function": {"name": "memory_eval"}},
                    "response_format": {"type": "json_object"},
                    "temperature": 0.25,
                    "max_tokens": 512,
                    "stream": false,
                    "seed": 42,
                    "strict": true
                }
            })
        })
        .collect();

    let client = Client::new();
    let batch_job = serde_json::json!({
        "input": jobs,
        "endpoint": "/v1/chat/completions"
    });

    let res = client
        .post("https://api.openai.com/v1/batch")
        .bearer_auth(api_key)
        .json(&batch_job)
        .send()
        .await?;

    if res.status() != StatusCode::OK {
        anyhow::bail!("Batch job failed: {:?}", res.text().await?);
    }
    let resp: serde_json::Value = res.json().await?;
    let batch_id = resp
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing batch ID"))?;

    // Poll batch status until done
    loop {
        let status_res = client
            .get(&format!(
                "https://api.openai.com/v1/batch/{}",
                batch_id
            ))
            .bearer_auth(api_key)
            .send()
            .await?;

        let status_json: serde_json::Value = status_res.json().await?;
        if status_json.get("status").and_then(|s| s.as_str()) == Some("completed") {
            break;
        }
        sleep(Duration::from_secs(5)).await;
    }

    // Download results
    let results_res = client
        .get(&format!(
            "https://api.openai.com/v1/batch/{}/output",
            batch_id
        ))
        .bearer_auth(api_key)
        .send()
        .await?;

    let results_json: serde_json::Value = results_res.json().await?;

    // Parse and align results to message IDs
    let mut eval_map = HashMap::new();
    for entry in results_json.as_array().unwrap_or(&vec![]) {
        if let (Some(custom_id), Some(out)) = (
            entry.get("custom_id").and_then(|v| v.as_str()),
            entry.get("response"),
        ) {
            // Parse memory_eval result
            if let Some(fn_result) = out
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("tool_calls"))
                .and_then(|tc| tc.get(0))
                .and_then(|tc| tc.get("function"))
                .and_then(|f| f.get("arguments"))
            {
                // Parse as MemoryEvalResult
                if let Ok(res) = serde_json::from_value::<MemoryEvalResult>(fn_result.clone()) {
                    eval_map.insert(custom_id.to_string(), res);
                }
            }
        }
    }

    Ok(eval_map)
}

/// Single-message fallback (not usually used, but good for testing)
pub async fn live_memory_eval(
    msg: &MiraMessage,
    api_key: &str,
) -> anyhow::Result<MemoryEvalResult> {
    let client = Client::new();

    let body = serde_json::json!({
        "model": "gpt-4.1",
        "messages": [
            {"role": &msg.role, "content": &msg.content}
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "memory_eval",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "persona": {"type": "string"},
                        "mood": {"type": "string"},
                        "salience": {"type": "number"},
                        "tags": {"type": "array", "items": {"type": "string"}},
                        "memory_type": {"type": "string"}
                    },
                    "required": ["persona", "mood", "salience", "tags", "memory_type"]
                }
            }
        }],
        "tool_choice": {"type": "function", "function": {"name": "memory_eval"}},
        "response_format": {"type": "json_object"},
        "temperature": 0.25,
        "max_tokens": 512,
        "stream": false,
        "seed": 42,
        "strict": true
    });

    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;

    if res.status() != StatusCode::OK {
        anyhow::bail!("Live memory_eval failed: {:?}", res.text().await?);
    }

    let resp_json: serde_json::Value = res.json().await?;
    if let Some(fn_result) = resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("tool_calls"))
        .and_then(|tc| tc.get(0))
        .and_then(|tc| tc.get("function"))
        .and_then(|f| f.get("arguments"))
    {
        let eval = serde_json::from_value::<MemoryEvalResult>(fn_result.clone())?;
        Ok(eval)
    } else {
        anyhow::bail!("No tool_calls result in response: {resp_json:#?}")
    }
}
