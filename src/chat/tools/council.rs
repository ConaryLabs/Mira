//! Council tool - consult multiple AI models in parallel
//!
//! Calls Opus 4.5, GPT 5.2, and Gemini 3 Pro in parallel to get diverse perspectives.

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio;

const DOTENV_PATH: &str = "/home/peter/Mira/.env";
const TIMEOUT_SECS: u64 = 120;

// API endpoints
const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";
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
// Anthropic API (Opus 4.5)
// ============================================================================

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    // Extended thinking - budget for internal reasoning
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
}

#[derive(Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    thinking_type: String,
    budget_tokens: u32,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Option<Vec<AnthropicContent>>,
    error: Option<AnthropicError>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicError {
    message: String,
}

async fn call_opus(message: &str) -> Result<String> {
    let api_key = get_env_var("ANTHROPIC_API_KEY")
        .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;

    let client = Client::new();

    let request = AnthropicRequest {
        model: "claude-opus-4-5-20251101".to_string(),
        max_tokens: 32000,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: message.to_string(),
        }],
        // Enable extended thinking with generous budget
        thinking: Some(ThinkingConfig {
            thinking_type: "enabled".to_string(),
            budget_tokens: 50000,
        }),
    };

    let response = client
        .post(ANTHROPIC_API_URL)
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Anthropic API error: {} - {}", status, body);
    }

    let api_response: AnthropicResponse = response.json().await?;

    if let Some(error) = api_response.error {
        anyhow::bail!("Anthropic error: {}", error.message);
    }

    let text = api_response
        .content
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.text)
        .unwrap_or_default();

    Ok(text)
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

async fn call_gpt(message: &str) -> Result<String> {
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

    Ok(text)
}

// ============================================================================
// Google API (Gemini 3 Pro)
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
struct GeminiError {
    message: String,
}

async fn call_gemini(message: &str) -> Result<String> {
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
                thinking_level: "high".to_string(),
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

    Ok(text)
}

// ============================================================================
// Council Tools
// ============================================================================

/// Council tool implementations - individual model calls and parallel council
pub struct CouncilTools;

impl CouncilTools {
    /// Ask GPT 5.2 directly
    pub async fn ask_gpt(message: &str, context: Option<&str>) -> Result<String> {
        let full_message = if let Some(ctx) = context {
            format!("Context: {}\n\n{}", ctx, message)
        } else {
            message.to_string()
        };

        let response = call_gpt(&full_message).await?;
        Ok(serde_json::json!({
            "provider": "gpt-5.2",
            "response": response
        }).to_string())
    }

    /// Ask Opus 4.5 directly
    pub async fn ask_opus(message: &str, context: Option<&str>) -> Result<String> {
        let full_message = if let Some(ctx) = context {
            format!("Context: {}\n\n{}", ctx, message)
        } else {
            message.to_string()
        };

        let response = call_opus(&full_message).await?;
        Ok(serde_json::json!({
            "provider": "opus-4.5",
            "response": response
        }).to_string())
    }

    /// Ask Gemini 3 Pro directly
    pub async fn ask_gemini(message: &str, context: Option<&str>) -> Result<String> {
        let full_message = if let Some(ctx) = context {
            format!("Context: {}\n\n{}", ctx, message)
        } else {
            message.to_string()
        };

        let response = call_gemini(&full_message).await?;
        Ok(serde_json::json!({
            "provider": "gemini-3-pro",
            "response": response
        }).to_string())
    }

    /// Call the council - all three models in parallel
    pub async fn council(message: &str, context: Option<&str>) -> Result<String> {
        let full_message = if let Some(ctx) = context {
            format!("Context: {}\n\n{}", ctx, message)
        } else {
            message.to_string()
        };

        // Run all three in parallel
        let (opus_result, gpt_result, gemini_result) = tokio::join!(
            call_opus(&full_message),
            call_gpt(&full_message),
            call_gemini(&full_message)
        );

        // Format responses, handling errors gracefully
        let opus = match opus_result {
            Ok(r) => r,
            Err(e) => format!("(error: {})", e),
        };
        let gpt = match gpt_result {
            Ok(r) => r,
            Err(e) => format!("(error: {})", e),
        };
        let gemini = match gemini_result {
            Ok(r) => r,
            Err(e) => format!("(error: {})", e),
        };

        // Return in the council format that chat.rs detects
        // Note: chat.rs looks for "council" key, then extracts individual model responses
        let result = json!({
            "council": {
                "gpt-5.2": gpt,
                "opus-4.5": opus,
                "gemini-3-pro": gemini,
            }
        });

        Ok(serde_json::to_string_pretty(&result)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires API keys
    async fn test_council() {
        let result = CouncilTools::council("What is 2+2?", None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("council"));
    }
}
