// src/llm/assistant/thread.rs

use crate::llm::client::OpenAIClient;
use reqwest::Method;
use serde::{Serialize, Deserialize};
use anyhow::{Result, Context};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Serialize, Debug, Default)]
pub struct CreateThreadRequest {
    // The Assistant API allows for optional initial messages or tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_resources: Option<ToolResources>,
    // Add more fields as needed.
}

#[derive(Serialize, Debug)]
pub struct ToolResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_store_ids: Option<Vec<String>>,
    // Add more as needed (code interpreter, web search, etc)
}

#[derive(Deserialize, Debug)]
pub struct ThreadResponse {
    pub id: String,
    // Add more fields if needed
}

pub struct ThreadManager {
    client: Arc<OpenAIClient>,
    threads: Arc<RwLock<HashMap<String, String>>>, // context_id/session_id → thread_id
}

impl ThreadManager {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self {
            client,
            threads: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or create a thread for the session/project.
    pub async fn get_or_create_thread(&self, context_id: &str) -> Result<String> {
        let mut threads = self.threads.write().await;
        if let Some(thread_id) = threads.get(context_id) {
            Ok(thread_id.clone())
        } else {
            let req = CreateThreadRequest::default();
            let res = self.client
                .request(Method::POST, "threads")
                .json(&req)
                .send()
                .await
                .context("Failed to send create thread request")?
                .error_for_status()
                .context("Non-2xx from OpenAI on create thread")?
                .json::<ThreadResponse>()
                .await
                .context("Failed to parse thread response")?;
            threads.insert(context_id.to_string(), res.id.clone());
            Ok(res.id)
        }
    }

    /// Get or create a thread with attached vector stores/tools.
    pub async fn get_or_create_thread_with_tools(
        &self,
        context_id: &str,
        vector_store_ids: Vec<String>,
    ) -> Result<String> {
        let mut threads = self.threads.write().await;
        if let Some(thread_id) = threads.get(context_id) {
            Ok(thread_id.clone())
        } else {
            let req = CreateThreadRequest {
                tool_resources: Some(ToolResources {
                    vector_store_ids: Some(vector_store_ids),
                }),
                ..Default::default()
            };
            let res = self.client
                .request(Method::POST, "threads")
                .json(&req)
                .send()
                .await
                .context("Failed to send create thread with tools request")?
                .error_for_status()
                .context("Non-2xx from OpenAI on create thread with tools")?
                .json::<ThreadResponse>()
                .await
                .context("Failed to parse thread response")?;
            threads.insert(context_id.to_string(), res.id.clone());
            Ok(res.id)
        }
    }
}
