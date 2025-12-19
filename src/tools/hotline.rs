// src/tools/hotline.rs
// Hotline - Talk to other AI models for collaboration/second opinion
// Supports: GPT-5.2 (default), DeepSeek V3.2, Gemini 3 Pro

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::types::HotlineRequest;

const MIRA_CHAT_URL: &str = "http://localhost:3001/api/chat/sync";
const DEFAULT_PROJECT_PATH: &str = "/home/peter/Mira";
const SYNC_TOKEN_ENV: &str = "MIRA_SYNC_TOKEN";
const DOTENV_PATH: &str = "/home/peter/Mira/.env";
const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-preview:generateContent";

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

/// Get Gemini API key from env var or .env file
fn get_gemini_key() -> Option<String> {
    // First try env var
    if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        return Some(key);
    }

    // Fallback: read from .env file
    if let Ok(contents) = std::fs::read_to_string(DOTENV_PATH) {
        for line in contents.lines() {
            if let Some(value) = line.strip_prefix("GEMINI_API_KEY=") {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
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

// Gemini API types
#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "thinkingConfig")]
    thinking_config: GeminiThinkingConfig,
    // No maxOutputTokens - let the model explain fully
}

#[derive(Serialize)]
struct GeminiThinkingConfig {
    #[serde(rename = "thinkingLevel")]
    thinking_level: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
    error: Option<GeminiError>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContentResponse,
}

#[derive(Deserialize)]
struct GeminiContentResponse {
    parts: Vec<GeminiPartResponse>,
}

#[derive(Deserialize)]
struct GeminiPartResponse {
    text: Option<String>,
}

#[derive(Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
}

#[derive(Deserialize)]
struct GeminiError {
    message: String,
}

/// Call Gemini 3 Pro directly
async fn call_gemini(message: &str) -> Result<serde_json::Value> {
    let api_key = get_gemini_key()
        .ok_or_else(|| anyhow::anyhow!("GEMINI_API_KEY not set"))?;

    let client = Client::new();
    let url = format!("{}?key={}", GEMINI_API_URL, api_key);

    let gemini_req = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![GeminiPart {
                text: message.to_string(),
            }],
        }],
        generation_config: Some(GeminiGenerationConfig {
            thinking_config: GeminiThinkingConfig {
                thinking_level: "high".to_string(),  // Maximum reasoning depth
            },
        }),
    };

    let response = client
        .post(&url)
        .json(&gemini_req)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Gemini API error: {} - {}", status, body);
    }

    let gemini_response: GeminiResponse = response.json().await?;

    // Check for API error
    if let Some(error) = gemini_response.error {
        anyhow::bail!("Gemini error: {}", error.message);
    }

    // Extract text from response
    let text = gemini_response
        .candidates
        .and_then(|c| c.into_iter().next())
        .map(|c| {
            c.content
                .parts
                .into_iter()
                .filter_map(|p| p.text)
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    let mut result = serde_json::json!({
        "response": text,
        "provider": "gemini",
    });

    if let Some(usage) = gemini_response.usage_metadata {
        result["tokens"] = serde_json::json!({
            "input": usage.prompt_token_count.unwrap_or(0),
            "output": usage.candidates_token_count.unwrap_or(0),
        });
    }

    Ok(result)
}

/// Call Mira via the mira-chat sync endpoint (or Gemini directly)
pub async fn call_mira(req: HotlineRequest) -> Result<serde_json::Value> {
    // Build message with optional context
    let message = if let Some(ctx) = &req.context {
        format!("Context: {}\n\n{}", ctx, req.message)
    } else {
        req.message.clone()
    };

    // Route to Gemini directly if requested (bypasses mira-chat)
    if req.provider.as_deref() == Some("gemini") {
        return call_gemini(&message).await;
    }

    // Otherwise, go through mira-chat sync endpoint
    let client = Client::new();

    let sync_req = SyncRequest {
        message,
        project_path: DEFAULT_PROJECT_PATH.to_string(),
        provider: req.provider,
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
            provider: None,
        };
        let result = call_mira(req).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires GEMINI_API_KEY
    async fn test_hotline_gemini() {
        let req = HotlineRequest {
            message: "What's 2+2?".to_string(),
            context: None,
            provider: Some("gemini".to_string()),
        };
        let result = call_mira(req).await;
        assert!(result.is_ok());
    }
}
