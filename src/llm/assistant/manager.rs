// src/llm/assistant/manager.rs

use crate::llm::client::OpenAIClient;
use reqwest::Method;
use serde::{Serialize, Deserialize};
use anyhow::{Result, Context};
use std::sync::Arc;

/// Request type for creating an assistant via OpenAI API.
#[derive(Serialize, Debug)]
pub struct CreateAssistantRequest {
    pub model: String,
    pub name: Option<String>,
    pub instructions: Option<String>,
    pub tools: Option<Vec<AssistantTool>>,
    pub temperature: Option<f32>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
pub enum AssistantTool {
    #[serde(rename = "file_search")]
    FileSearch { max_num_results: Option<u32> },
    // Add more tool types as you extend Mira!
}

#[derive(Deserialize, Debug)]
pub struct AssistantResponse {
    pub id: String,
    // Add more fields from OpenAI API as needed.
}

pub struct AssistantManager {
    client: Arc<OpenAIClient>,
    pub assistant_id: Option<String>,
}

impl AssistantManager {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self {
            client,
            assistant_id: None,
        }
    }

    /// Create a new OpenAI Assistant for Mira with file search enabled.
    pub async fn create_assistant(&mut self) -> Result<()> {
        let req = CreateAssistantRequest {
            model: "gpt-4.1".to_string(),
            name: Some("Mira with Vector Stores".to_string()),
            instructions: Some(include_str!("../../../prompts/mira_instructions.txt").to_string()),
            tools: Some(vec![AssistantTool::FileSearch { max_num_results: Some(20) }]),
            temperature: Some(0.9),
        };

        let res = self.client
            .request(Method::POST, "assistants")
            .json(&req)
            .send()
            .await
            .context("Failed to send create assistant request")?
            .error_for_status()
            .context("Non-2xx from OpenAI when creating assistant")?
            .json::<AssistantResponse>()
            .await
            .context("Failed to parse create assistant response")?;

        self.assistant_id = Some(res.id);
        Ok(())
    }

    /// Optionally: get the assistant ID (for use in threads/vector store modules)
    pub fn get_assistant_id(&self) -> Option<&str> {
        self.assistant_id.as_deref()
    }
}
