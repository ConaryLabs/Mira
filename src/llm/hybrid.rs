// src/services/hybrid.rs

use crate::llm::assistant::{AssistantManager, VectorStoreManager, ThreadManager};
use crate::llm::persona::PersonaOverlay;
use crate::services::{ChatService, MemoryService, ContextService};
use crate::memory::types::MemoryEntry;
use crate::llm::schema::{ChatResponse, MemoryType};
use chrono::Utc;
use anyhow::{Result, Context as AnyhowContext};
use std::sync::Arc;
use reqwest::Method;
use serde::{Serialize, Deserialize};
use tokio::time::{sleep, Duration};
use std::time::Instant;

#[derive(Serialize)]
struct CreateMessageRequest {
    pub role: String,
    pub content: String,
    // You could add additional fields like `attachments` here if needed.
}

#[derive(Deserialize)]
struct MessageResponse {
    pub id: String,
    // ... more as needed
}

#[derive(Serialize)]
struct CreateRunRequest {
    pub assistant_id: String,
    // Optionally, you could set instructions, tools, etc.
}

#[derive(Deserialize)]
struct RunResponse {
    pub id: String,
    pub status: String,
    // ... more as needed
}

#[derive(Deserialize)]
struct RunStatusResponse {
    pub status: String,
    pub last_error: Option<RunError>,
    // ... more as needed
}

#[derive(Deserialize)]
struct RunError {
    pub message: Option<String>,
    // ... more as needed
}

#[derive(Deserialize)]
struct ListMessagesResponse {
    pub data: Vec<ThreadMessage>,
}

#[derive(Deserialize)]
struct ThreadMessage {
    pub role: String,
    pub content: Vec<ThreadContent>,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "text", rename_all = "snake_case")]
enum ThreadContent {
    Text(String),
    // Add more variants if using images, etc.
}

pub struct HybridMemoryService {
    chat_service: Arc<ChatService>,
    memory_service: Arc<MemoryService>,
    context_service: Arc<ContextService>,
    assistant_manager: Arc<AssistantManager>,
    vector_store_manager: Arc<VectorStoreManager>,
    thread_manager: Arc<ThreadManager>,
}

impl HybridMemoryService {
    pub fn new(
        chat_service: Arc<ChatService>,
        memory_service: Arc<MemoryService>,
        context_service: Arc<ContextService>,
        assistant_manager: Arc<AssistantManager>,
        vector_store_manager: Arc<VectorStoreManager>,
        thread_manager: Arc<ThreadManager>,
    ) -> Self {
        Self {
            chat_service,
            memory_service,
            context_service,
            assistant_manager,
            vector_store_manager,
            thread_manager,
        }
    }

    pub async fn process_with_hybrid_memory(
        &self,
        session_id: &str,
        content: &str,
        persona: &PersonaOverlay,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        let thread_id = if let Some(proj_id) = project_id {
            self.ensure_thread_with_vector_store(session_id, proj_id).await?
        } else {
            self.thread_manager.get_or_create_thread(session_id).await?
        };

        let embedding = self.chat_service.llm_client
            .get_embedding(content)
            .await
            .ok();

        let personal_context = self.context_service
            .build_context(session_id, embedding.as_deref(), project_id)
            .await?;

        let enriched_message = self.enrich_message_with_context(content, &personal_context);

        let assistant_response = self
            .run_assistant_with_context(&thread_id, &enriched_message, persona)
            .await?;

        self.sync_insights_to_personal_memory(
            session_id,
            &assistant_response,
            project_id,
        ).await?;

        Ok(assistant_response)
    }

    async fn ensure_thread_with_vector_store(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> Result<String> {
        let vector_store_id = self.vector_store_manager
            .create_project_store(project_id)
            .await?;

        self.thread_manager
            .get_or_create_thread_with_tools(
                session_id,
                vec![vector_store_id],
            )
            .await
    }

    fn enrich_message_with_context(
        &self,
        content: &str,
        context: &str,
    ) -> String {
        if context.is_empty() {
            content.to_string()
        } else {
            format!("{}\n\n[Personal Context:]\n{}", content, context)
        }
    }

    /// FULLY IMPLEMENTED: Posts message to assistant thread, runs, polls for completion, and extracts LLM output.
    async fn run_assistant_with_context(
        &self,
        thread_id: &str,
        message: &str,
        _persona: &PersonaOverlay,
    ) -> Result<ChatResponse> {
        let client = &self.chat_service.llm_client;

        // 1. Post message to thread
        let msg_req = CreateMessageRequest {
            role: "user".to_string(),
            content: message.to_string(),
        };
        let msg_res = client
            .request(Method::POST, &format!("threads/{}/messages", thread_id))
            .json(&msg_req)
            .send()
            .await
            .context("Failed to post message to thread")?
            .error_for_status()
            .context("Non-2xx from OpenAI when posting message")?
            .json::<MessageResponse>()
            .await
            .context("Failed to parse thread message response")?;

        // 2. Start a run
        let assistant_id = self
            .assistant_manager
            .get_assistant_id()
            .ok_or_else(|| anyhow::anyhow!("Assistant ID not available"))?
            .to_string();

        let run_req = CreateRunRequest {
            assistant_id,
        };
        let run_res = client
            .request(Method::POST, &format!("threads/{}/runs", thread_id))
            .json(&run_req)
            .send()
            .await
            .context("Failed to start assistant run")?
            .error_for_status()
            .context("Non-2xx from OpenAI when starting run")?
            .json::<RunResponse>()
            .await
            .context("Failed to parse run response")?;

        // 3. Poll for run completion
        let poll_start = Instant::now();
        let mut status = run_res.status.clone();
        let mut last_error: Option<RunError> = None;
        let mut run_id = run_res.id.clone();
        let max_wait = Duration::from_secs(60);

        while status != "completed" && status != "failed" && poll_start.elapsed() < max_wait {
            sleep(Duration::from_millis(900)).await;

            let run_status = client
                .request(Method::GET, &format!("threads/{}/runs/{}", thread_id, run_id))
                .send()
                .await
                .context("Failed to poll run status")?
                .error_for_status()
                .context("Non-2xx from OpenAI when polling run status")?
                .json::<RunStatusResponse>()
                .await
                .context("Failed to parse run status response")?;

            status = run_status.status;
            last_error = run_status.last_error;
        }

        if status != "completed" {
            let msg = last_error
                .and_then(|e| e.message)
                .unwrap_or_else(|| "Unknown error or timeout from Assistant API".to_string());
            return Err(anyhow::anyhow!(
                "Assistant run failed or timed out: {}",
                msg
            ));
        }

        // 4. Fetch latest messages from the thread
        let msgs = client
            .request(Method::GET, &format!("threads/{}/messages?limit=10", thread_id))
            .send()
            .await
            .context("Failed to fetch thread messages")?
            .error_for_status()
            .context("Non-2xx from OpenAI when fetching messages")?
            .json::<ListMessagesResponse>()
            .await
            .context("Failed to parse messages response")?;

        // 5. Extract assistant's response (latest non-user message)
        let llm_message = msgs.data
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .and_then(|m| m.content.iter().find_map(|c| match c {
                ThreadContent::Text(txt) => Some(txt),
                //_ => None,
            }))
            .cloned()
            .unwrap_or_else(|| "No assistant response found".to_string());

        // 6. Return as ChatResponse (populate only output for now; you can expand as needed)
        Ok(ChatResponse {
            output: llm_message,
            persona: Some("Assistant".to_string()),
            mood: None,
            salience: 5, // You can add LLM-driven scoring here if desired
            tags: vec![],
            memory_type: None,
            intent: None,
            summary: None,
            monologue: None,
        })
    }

    async fn sync_insights_to_personal_memory(
        &self,
        session_id: &str,
        response: &ChatResponse,
        project_id: Option<&str>,
    ) -> Result<()> {
        if response.salience >= 7 {
            let insight = MemoryEntry {
                id: None,
                session_id: session_id.to_string(),
                role: "system".to_string(),
                content: format!(
                    "Insight from project {}: {}",
                    project_id.unwrap_or("general"),
                    response.summary.as_ref().unwrap_or(&response.output)
                ),
                timestamp: Utc::now(),
                embedding: self.chat_service.llm_client
                    .get_embedding(&response.output)
                    .await
                    .ok(),
                salience: Some(response.salience as f32),
                tags: Some(vec!["insight".to_string(), "synced".to_string()]),
                memory_type: Some(MemoryType::Event),
                ..Default::default()
            };
            self.memory_service.qdrant_store.save(&insight).await?;
        }
        Ok(())
    }
}
