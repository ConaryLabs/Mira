// src/tools/hotline.rs
// Hotline - Talk to Mira (GPT-5.2) via mira-chat sync endpoint

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::types::HotlineRequest;

const MIRA_CHAT_URL: &str = "http://localhost:3001/api/chat/sync";
const DEFAULT_PROJECT_PATH: &str = "/home/peter/Mira";
const SYNC_TOKEN_ENV: &str = "MIRA_SYNC_TOKEN";
const DOTENV_PATH: &str = "/home/peter/Mira/.env";

/// Get sync token from env var or .env file
fn get_sync_token() -> Option<String> {
    // First try env var
    if let Ok(token) = std::env::var(SYNC_TOKEN_ENV) {
        return Some(token);
    }

    // Fallback: read from .env file
    if let Ok(contents) = std::fs::read_to_string(DOTENV_PATH) {
        for line in contents.lines() {
            if let Some(value) = line.strip_prefix("MIRA_SYNC_TOKEN=") {
                return Some(value.trim().to_string());
            }
        }
    }

    None
}

#[derive(Serialize)]
struct SyncRequest {
    message: String,
    project_path: String,
}

#[derive(Deserialize)]
struct SyncResponse {
    content: String,
    #[serde(default)]
    tool_calls: Vec<serde_json::Value>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u32,
    output_tokens: u32,
}

/// Call Mira via the mira-chat sync endpoint
pub async fn call_mira(req: HotlineRequest) -> Result<serde_json::Value> {
    let client = Client::new();

    // Build message with optional context
    let message = if let Some(ctx) = req.context {
        format!("Context: {}\n\n{}", ctx, req.message)
    } else {
        req.message
    };

    let sync_req = SyncRequest {
        message,
        project_path: DEFAULT_PROJECT_PATH.to_string(),
    };

    // Build request with optional auth token
    let mut request = client
        .post(MIRA_CHAT_URL)
        .json(&sync_req)
        .timeout(std::time::Duration::from_secs(120));

    // Add Bearer token if available (env var or .env file)
    if let Some(token) = get_sync_token() {
        request = request.bearer_auth(token);
    }

    let response = request.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Mira hotline error: {} - {}", status, body);
    }

    let sync_response: SyncResponse = response.json().await?;

    let mut result = serde_json::json!({
        "response": sync_response.content,
    });

    if !sync_response.tool_calls.is_empty() {
        result["tool_calls"] = serde_json::json!(sync_response.tool_calls.len());
    }

    if let Some(usage) = sync_response.usage {
        result["tokens"] = serde_json::json!({
            "input": usage.input_tokens,
            "output": usage.output_tokens,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires mira-chat running
    async fn test_hotline() {
        let req = HotlineRequest {
            message: "What's 2+2?".to_string(),
            context: None,
        };
        let result = call_mira(req).await;
        assert!(result.is_ok());
    }
}
