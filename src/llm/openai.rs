// src/llm/openai.rs

use serde_json::json;
use reqwest::Client;
use std::env;

/// Sends a prompt and user message to OpenAI GPT-4.1 with the provided function schema.
/// Returns the full JSON result from OpenAI.
pub async fn call_openai_with_function(
    prompt: &str,
    user_message: &str,
    function_schema: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let api_key = env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
    let client = Client::new();

    let body = json!({
        "model": "gpt-4.1",
        "messages": [
            { "role": "system", "content": prompt },
            { "role": "user",   "content": user_message }
        ],
        "functions": function_schema,
        "function_call": { "name": "format_response" }
    });

    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("OpenAI call failed: {e:?}"))?;

    let result: serde_json::Value = res.json().await.map_err(|e| format!("OpenAI output parse error: {e:?}"))?;
    Ok(result)
}
