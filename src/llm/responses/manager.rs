// src/llm/responses/manager.rs - Modern Responses API, no tool support

use crate::llm::client::OpenAIClient;
use reqwest::Method;
use serde::{Serialize, Deserialize};
use anyhow::{Result, Context};
use std::sync::Arc;

/// Request for creating a response (no tools, context injected via messages)
#[derive(Serialize, Debug)]
pub struct CreateResponseRequest {
    pub model: String,
    pub messages: Vec<ResponseMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResponseMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize, Debug)]
pub struct ResponseObject {
    pub id: String,
    pub object: String,
    pub model: String,
    pub choices: Vec<ResponseChoice>,
    pub created_at: Option<i64>, // <-- Now optional
}

#[derive(Deserialize, Debug)]
pub struct ResponseChoice {
    pub index: i32,
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

/// Manager for the new Responses API (tooling deprecated)
pub struct ResponsesManager {
    client: Arc<OpenAIClient>,
    pub responses_id: Option<String>,
}

impl ResponsesManager {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self {
            client,
            responses_id: Some("responses-api-v1".to_string()),
        }
    }

    /// "Create responses" now just returns success since we use Responses API
    pub async fn create_responses(&mut self) -> Result<()> {
        eprintln!("🤖 Initializing Responses API manager (no responses creation needed)...");
        Ok(())
    }

    /// Create a response using OpenAI Responses API (no tools/context only)
    pub async fn create_response(
        &self,
        messages: Vec<ResponseMessage>,
    ) -> Result<ResponseObject> {
        let req = CreateResponseRequest {
            model: "gpt-4.1".to_string(), // Update to latest model if needed
            messages,
            temperature: Some(0.3),
        };

        eprintln!("📤 Creating response via Responses API");

        let response = self.client
            .request(Method::POST, "chat/completions")
            .json(&req)
            .send()
            .await
            .context("Failed to send response request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            eprintln!("❌ Responses API error: {}", error_text);
            return Err(anyhow::anyhow!("HTTP {} from OpenAI: {}", status, error_text));
        }

        let result: ResponseObject = response
            .json()
            .await
            .context("Failed to parse response")?;

        Ok(result)
    }

    /// Get the responses ID (kept for compatibility)
    pub fn get_responses_id(&self) -> Option<&str> {
        self.responses_id.as_deref()
    }
}
