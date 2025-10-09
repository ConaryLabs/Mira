// src/llm/moderation.rs

use anyhow::{Context, Result};
use serde_json::Value;
use serde_json::json;
use reqwest::Client;

#[derive(Debug)]
pub struct ModerationResult {
    pub flagged: bool,
    pub categories: Vec<String>, // only categories that evaluated to true
}

/// Run OpenAI Moderation on `text`.
/// Warn‑only: returns `Ok(None)` on API issues; never blocks user flow.
pub async fn moderate(api_key: &str, text: &str) -> Result<Option<ModerationResult>> {
    let model = std::env::var("MODERATION_MODEL")
        .unwrap_or_else(|_| "omni-moderation-latest".to_string());

    let body = json!({
        "model": model,
        "input": text,
    });

    let client = Client::new();
    let resp = client
        .post("https://api.openai.com/v1/moderations")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("OpenAI moderation request failed: {}", e);
            return Ok(None);
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        tracing::warn!("OpenAI moderation error {}: {}", status, text);
        return Ok(None);
    }

    let v: Value = resp
        .json()
        .await
        .context("Invalid JSON from /v1/moderations")?;

    // Shape (typical):
    // {
    //   "id": "...",
    //   "model": "...",
    //   "results": [
    //     {
    //       "flagged": bool,
    //       "categories": { "violence": bool, "hate": bool, ... },
    //       "category_scores": { ... }
    //     }
    //   ]
    // }
    let first = v
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first());

    let flagged = first
        .and_then(|o| o.get("flagged"))
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let mut categories = Vec::new();
    if let Some(obj) = first.and_then(|o| o.get("categories")).and_then(|c| c.as_object()) {
        for (k, val) in obj {
            if val.as_bool().unwrap_or(false) {
                categories.push(k.clone());
            }
        }
    }

    if flagged {
        tracing::warn!("MODERATION (WARN ONLY): flagged categories: {:?}", categories);
    }

    Ok(Some(ModerationResult { flagged, categories }))
}
