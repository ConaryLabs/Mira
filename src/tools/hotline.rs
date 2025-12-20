// src/tools/hotline.rs
// Hotline - Talk to other AI models for collaboration/second opinion
// Supports: GPT-5.2 (default), DeepSeek V3.2, Gemini 3 Pro
// All providers are called directly via their APIs (no mira-chat dependency)

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::types::HotlineRequest;

const DOTENV_PATH: &str = "/home/peter/Mira/.env";
const TIMEOUT_SECS: u64 = 120;

// API endpoints
const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/v1/chat/completions";
const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-preview:generateContent";

// ============================================================================
// Environment helpers
// ============================================================================

fn get_env_var(name: &str) -> Option<String> {
    // First try env var
    if let Ok(val) = std::env::var(name) {
        return Some(val);
    }

    // Fallback: read from .env file
    if let Ok(contents) = std::fs::read_to_string(DOTENV_PATH) {
        let prefix = format!("{}=", name);
        for line in contents.lines() {
            if let Some(value) = line.strip_prefix(&prefix) {
                return Some(value.trim().to_string());
            }
        }
    }

    None
}

// ============================================================================
// OpenAI API (GPT 5.2)
// ============================================================================

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_completion_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

#[derive(Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Option<Vec<OpenAIChoice>>,
    error: Option<OpenAIError>,
    usage: Option<OpenAIUsage>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessageResponse,
}

#[derive(Deserialize)]
struct OpenAIMessageResponse {
    content: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIError {
    message: String,
}

#[derive(Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

async fn call_gpt(message: &str) -> Result<serde_json::Value> {
    let api_key = get_env_var("OPENAI_API_KEY")
        .ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

    let client = Client::new();

    let request = OpenAIRequest {
        model: "gpt-5.2".to_string(),
        messages: vec![OpenAIMessage {
            role: "user".to_string(),
            content: message.to_string(),
        }],
        max_completion_tokens: 32000,
        reasoning_effort: Some("high".to_string()),
    };

    let response = client
        .post(OPENAI_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI API error: {} - {}", status, body);
    }

    let api_response: OpenAIResponse = response.json().await?;

    if let Some(error) = api_response.error {
        anyhow::bail!("OpenAI error: {}", error.message);
    }

    let text = api_response
        .choices
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.message.content)
        .unwrap_or_default();

    let mut result = serde_json::json!({
        "response": text,
        "provider": "gpt-5.2",
    });

    if let Some(usage) = api_response.usage {
        result["tokens"] = serde_json::json!({
            "input": usage.prompt_tokens,
            "output": usage.completion_tokens,
        });
    }

    Ok(result)
}

// ============================================================================
// DeepSeek API (V3.2)
// ============================================================================

#[derive(Serialize)]
struct DeepSeekRequest {
    model: String,
    messages: Vec<DeepSeekMessage>,
    max_tokens: u32,
}

#[derive(Serialize)]
struct DeepSeekMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct DeepSeekResponse {
    choices: Option<Vec<DeepSeekChoice>>,
    error: Option<DeepSeekError>,
    usage: Option<DeepSeekUsage>,
}

#[derive(Deserialize)]
struct DeepSeekChoice {
    message: DeepSeekMessageResponse,
}

#[derive(Deserialize)]
struct DeepSeekMessageResponse {
    content: Option<String>,
}

#[derive(Deserialize)]
struct DeepSeekError {
    message: String,
}

#[derive(Deserialize)]
struct DeepSeekUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

async fn call_deepseek(message: &str) -> Result<serde_json::Value> {
    let api_key = get_env_var("DEEPSEEK_API_KEY")
        .ok_or_else(|| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;

    let client = Client::new();

    let request = DeepSeekRequest {
        model: "deepseek-chat".to_string(),
        messages: vec![DeepSeekMessage {
            role: "user".to_string(),
            content: message.to_string(),
        }],
        max_tokens: 32000,
    };

    let response = client
        .post(DEEPSEEK_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("DeepSeek API error: {} - {}", status, body);
    }

    let api_response: DeepSeekResponse = response.json().await?;

    if let Some(error) = api_response.error {
        anyhow::bail!("DeepSeek error: {}", error.message);
    }

    let text = api_response
        .choices
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.message.content)
        .unwrap_or_default();

    let mut result = serde_json::json!({
        "response": text,
        "provider": "deepseek",
    });

    if let Some(usage) = api_response.usage {
        result["tokens"] = serde_json::json!({
            "input": usage.prompt_tokens,
            "output": usage.completion_tokens,
        });
    }

    Ok(result)
}

// ============================================================================
// Google API (Gemini 2.5 Pro)
// ============================================================================

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

async fn call_gemini(message: &str) -> Result<serde_json::Value> {
    let api_key = get_env_var("GEMINI_API_KEY")
        .ok_or_else(|| anyhow::anyhow!("GEMINI_API_KEY not set"))?;

    let client = Client::new();
    let url = format!("{}?key={}", GEMINI_API_URL, api_key);

    let request = GeminiRequest {
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
        .json(&request)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Gemini API error: {} - {}", status, body);
    }

    let api_response: GeminiResponse = response.json().await?;

    if let Some(error) = api_response.error {
        anyhow::bail!("Gemini error: {}", error.message);
    }

    let text = api_response
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

    if let Some(usage) = api_response.usage_metadata {
        result["tokens"] = serde_json::json!({
            "input": usage.prompt_token_count.unwrap_or(0),
            "output": usage.candidates_token_count.unwrap_or(0),
        });
    }

    Ok(result)
}

// ============================================================================
// Council - All models in parallel
// ============================================================================

async fn call_council(message: &str) -> Result<serde_json::Value> {
    // Run all three in parallel
    let (gpt_result, deepseek_result, gemini_result) = tokio::join!(
        call_gpt(message),
        call_deepseek(message),
        call_gemini(message)
    );

    // Format responses, handling errors gracefully
    let gpt = match gpt_result {
        Ok(r) => r["response"].as_str().unwrap_or("(error)").to_string(),
        Err(e) => format!("(error: {})", e),
    };
    let deepseek = match deepseek_result {
        Ok(r) => r["response"].as_str().unwrap_or("(error)").to_string(),
        Err(e) => format!("(error: {})", e),
    };
    let gemini = match gemini_result {
        Ok(r) => r["response"].as_str().unwrap_or("(error)").to_string(),
        Err(e) => format!("(error: {})", e),
    };

    Ok(serde_json::json!({
        "council": {
            "gpt-5.2": gpt,
            "deepseek": deepseek,
            "gemini": gemini,
        }
    }))
}

// ============================================================================
// Public API
// ============================================================================

/// Call Mira hotline - talk to another AI model
/// Providers: openai (GPT-5.2, default), deepseek, gemini, council (all three)
pub async fn call_mira(req: HotlineRequest) -> Result<serde_json::Value> {
    // Build message with optional context
    let message = if let Some(ctx) = &req.context {
        format!("Context: {}\n\n{}", ctx, req.message)
    } else {
        req.message.clone()
    };

    // Route based on provider
    match req.provider.as_deref() {
        Some("gemini") => call_gemini(&message).await,
        Some("deepseek") => call_deepseek(&message).await,
        Some("council") => call_council(&message).await,
        _ => call_gpt(&message).await, // default to GPT-5.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires OPENAI_API_KEY
    async fn test_hotline_gpt() {
        let req = HotlineRequest {
            message: "What's 2+2?".to_string(),
            context: None,
            provider: None,
        };
        let result = call_mira(req).await;
        assert!(result.is_ok());
        println!("GPT result: {:?}", result);
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
        println!("Gemini result: {:?}", result);
    }

    #[tokio::test]
    #[ignore] // Requires all API keys
    async fn test_council() {
        let req = HotlineRequest {
            message: "What's 2+2?".to_string(),
            context: None,
            provider: Some("council".to_string()),
        };
        let result = call_mira(req).await;
        assert!(result.is_ok());
        println!("Council result: {:?}", result);
    }
}
