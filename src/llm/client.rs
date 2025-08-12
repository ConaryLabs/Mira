// src/llm/client.rs
use anyhow::{Context, Result};
use reqwest::{Client, Method, RequestBuilder, Response};
use serde_json::{json, Value};
use std::env;

/// Thin OpenAI HTTP client shared across services.
/// - Targets the unified **/v1/responses** endpoint for text/tools/images
/// - Keeps a generic JSON request builder for other endpoints (embeddings/moderation)
#[derive(Clone)]
pub struct OpenAIClient {
    pub client: Client,
    pub api_key: String,
    pub api_base: String,
}

impl OpenAIClient {
    /// Construct from OPENAI_API_KEY and optional OPENAI_BASE_URL.
    pub fn new() -> Result<Self> {
        let api_key = env::var("OPENAI_API_KEY").context("OPENAI_API_KEY not set")?;
        let api_base = env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        Ok(Self {
            client: Client::new(),
            api_key,
            api_base,
        })
    }

    /// Some modules build their own reqwest calls; expose the standard auth header.
    pub fn auth_header(&self) -> (&'static str, String) {
        ("Authorization", format!("Bearer {}", self.api_key))
    }

    /// Build an authenticated JSON request to `{api_base}/{path}`.
    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.client
            .request(
                method,
                format!(
                    "{}/{}",
                    self.api_base.trim_end_matches('/'),
                    path.trim_start_matches('/')
                ),
            )
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
    }

    /// Multipart builder for upload endpoints (content type set by reqwest).
    pub fn request_multipart(&self, path: &str) -> RequestBuilder {
        self.client
            .post(format!(
                "{}/{}",
                self.api_base.trim_end_matches('/'),
                path.trim_start_matches('/')
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
    }

    // -------------------------------
    // Responses API helpers
    // -------------------------------

    /// POST a unified **/responses** request body and return parsed JSON.
    pub async fn post_response(&self, body: Value) -> Result<Value> {
        let res = self
            .request(Method::POST, "responses")
            .json(&body)
            .send()
            .await
            .context("Failed to POST /responses")?;
        Self::ok_json(res).await
    }

    /// Like `post_response` but returns the raw `Response` (for streaming).
    pub async fn post_response_raw(&self, body: &Value) -> Result<Response> {
        let res = self
            .request(Method::POST, "responses")
            .json(body)
            .send()
            .await
            .context("Failed to POST /responses")?;
        Ok(res)
    }

    async fn ok_json(res: Response) -> Result<Value> {
        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OpenAI error {}: {}",
                status.as_u16(),
                text
            ));
        }
        let v: Value = res.json().await.context("Invalid JSON from OpenAI")?;
        Ok(v)
    }
}

/// Legacy helper used by `src/llm/chat.rs`.
/// Returns combined assistant text from either the unified `output` array
/// (preferred) or the `choices[0].message.content` fallback.
pub fn extract_text_from_responses(v: &Value) -> Option<String> {
    // Prefer unified `output` array (Responses API)
    if let Some(arr) = v.get("output").and_then(|o| o.as_array()) {
        let mut s = String::new();
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                    s.push_str(t);
                }
            }
        }
        if !s.is_empty() {
            return Some(s);
        }
    }

    // Fallback: choices[0].message.content parts (compat)
    if let Some(parts) = v
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_array())
    {
        let mut s = String::new();
        for part in parts {
            if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                    s.push_str(t);
                }
            }
        }
        if !s.is_empty() {
            return Some(s);
        }
    }

    None
}
