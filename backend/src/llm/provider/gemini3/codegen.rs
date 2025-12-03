// src/llm/provider/gemini3/codegen.rs
// Code generation logic for Gemini 3

use super::types::{CodeArtifact, CodeGenRequest, CodeGenResponse};
use super::response::log_codegen_tokens;
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::Value;
use tracing::info;

use crate::prompt::internal::llm as prompts;

/// Build user prompt from code generation request
pub fn build_user_prompt(request: &CodeGenRequest) -> String {
    let mut prompt = format!(
        "Generate a {} file at path: {}\n\n\
        Description: {}\n\n",
        request.language, request.path, request.description
    );

    if let Some(framework) = &request.framework {
        prompt.push_str(&format!("Framework: {}\n\n", framework));
    }

    if !request.dependencies.is_empty() {
        prompt.push_str(&format!(
            "Dependencies: {}\n\n",
            request.dependencies.join(", ")
        ));
    }

    if let Some(style) = &request.style_guide {
        prompt.push_str(&format!("Style preferences: {}\n\n", style));
    }

    if !request.context.is_empty() {
        prompt.push_str(&format!("Additional context:\n{}\n\n", request.context));
    }

    prompt.push_str("Remember: Output ONLY the JSON object, no other text.");

    prompt
}

/// Execute code generation request
pub async fn generate_code(
    client: &Client,
    api_url: &str,
    request: CodeGenRequest,
) -> Result<CodeGenResponse> {
    info!(
        "Gemini 3: Generating {} code at {}",
        request.language, request.path
    );

    let system_prompt = prompts::code_gen_specialist(&request.language);
    let user_prompt = build_user_prompt(&request);

    let request_body = serde_json::json!({
        "contents": [{
            "role": "user",
            "parts": [{
                "text": format!("{}\n\n{}", system_prompt, user_prompt)
            }]
        }],
        "generationConfig": {
            "temperature": 1.0,
            "responseMimeType": "application/json"
        }
    });

    let response = client
        .post(api_url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow!("Gemini 3 API error {}: {}", status, error_text));
    }

    let response_json: Value = response.json().await?;

    // Extract content from response
    let content_str = response_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Invalid Gemini 3 response structure"))?;

    // Parse the JSON content
    let artifact: CodeArtifact = serde_json::from_str(content_str)?;

    // Extract token usage
    let usage = response_json.get("usageMetadata");
    let tokens_input = usage
        .and_then(|u| u.get("promptTokenCount"))
        .and_then(|t| t.as_i64())
        .unwrap_or(0);
    let tokens_output = usage
        .and_then(|u| u.get("candidatesTokenCount"))
        .and_then(|t| t.as_i64())
        .unwrap_or(0);
    let tokens_cached = usage
        .and_then(|u| u.get("cachedContentTokenCount"))
        .and_then(|t| t.as_i64())
        .unwrap_or(0);

    log_codegen_tokens(
        "Gemini 3",
        artifact.content.lines().count(),
        &artifact.path,
        tokens_input,
        tokens_cached,
    );

    Ok(CodeGenResponse {
        artifact,
        tokens_input,
        tokens_output,
    })
}
