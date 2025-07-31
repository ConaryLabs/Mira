// src/llm/assistant/manager.rs - Updated to use Responses API

use crate::llm::client::OpenAIClient;
use reqwest::Method;
use serde::{Serialize, Deserialize};
use anyhow::{Result, Context};
use std::sync::Arc;

/// Request for creating a response with file search
#[derive(Serialize, Debug)]
pub struct CreateResponseRequest {
    pub model: String,
    pub messages: Vec<ResponseMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_resources: Option<ToolResources>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResponseMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
pub enum Tool {
    #[serde(rename = "file_search")]
    FileSearch,
}

#[derive(Serialize, Debug)]
pub struct ToolResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_search: Option<FileSearchResources>,
}

#[derive(Serialize, Debug)]
pub struct FileSearchResources {
    pub vector_store_ids: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct ResponseObject {
    pub id: String,
    pub object: String,
    pub created_at: i64,
    pub model: String,
    pub choices: Vec<ResponseChoice>,
}

#[derive(Deserialize, Debug)]
pub struct ResponseChoice {
    pub index: i32,
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

/// Manager for the new Responses API
pub struct AssistantManager {
    client: Arc<OpenAIClient>,
    pub assistant_id: Option<String>,
}

impl AssistantManager {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self {
            client,
            // We don't actually create an assistant anymore
            assistant_id: Some("responses-api-v1".to_string()),
        }
    }

    /// "Create assistant" now just returns success since we use Responses API
    pub async fn create_assistant(&mut self) -> Result<()> {
        eprintln!("🤖 Initializing Responses API manager (no assistant creation needed)...");
        // No actual assistant to create with the new API
        // We'll use the responses endpoint directly
        Ok(())
    }

    /// Create a response with access to vector stores
    pub async fn create_response_with_vector_stores(
        &self,
        messages: Vec<ResponseMessage>,
        vector_store_ids: Vec<String>,
    ) -> Result<ResponseObject> {
        let req = CreateResponseRequest {
            model: "gpt-4.1".to_string(),
            messages,
            tools: Some(vec![Tool::FileSearch]),
            tool_resources: Some(ToolResources {
                file_search: Some(FileSearchResources {
                    vector_store_ids,
                }),
            }),
            temperature: Some(0.3),
        };

        eprintln!("📤 Creating response with {} vector stores", 
            req.tool_resources.as_ref()
                .and_then(|tr| tr.file_search.as_ref())
                .map(|fs| fs.vector_store_ids.len())
                .unwrap_or(0)
        );

        let response = self.client
            .request(Method::POST, "chat/completions") // Using chat completions with tools
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

    /// Get the assistant ID (kept for compatibility)
    pub fn get_assistant_id(&self) -> Option<&str> {
        self.assistant_id.as_deref()
    }
}
