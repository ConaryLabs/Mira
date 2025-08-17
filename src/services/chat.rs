// src/services/chat.rs
// Final version with borrow checker fixes and cleanup

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{info, debug, instrument};
use futures::StreamExt;

use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::{ThreadManager, ResponseMessage};
use crate::llm::responses::vector_store::VectorStoreManager;
use crate::llm::streaming::StreamEvent;
use crate::services::memory::MemoryService;
use crate::services::summarization::SummarizationService;
use crate::persona::PersonaOverlay;
use crate::memory::recall::RecallContext;
use crate::api::two_phase::{get_metadata, get_content_stream};
use crate::api::types::ResponseMetadata;

/// Output format for chat responses
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: usize,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

/// Configuration for ChatService
#[derive(Clone)]
pub struct ChatConfig {
    pub model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
    pub history_message_cap: usize,
    pub history_token_limit: usize,
    pub max_retrieval_tokens: usize,
    pub max_vector_search_results: usize,
    pub enable_vector_search: bool,
    pub enable_web_search: bool,
    pub enable_code_interpreter: bool,
    pub enable_debug_logging: bool,
    pub enable_summarization: bool,
    pub summary_chunk_size: usize,
    pub summary_token_limit: usize,
    pub summary_output_tokens: usize,
}

impl ChatConfig {
    pub fn from_env() -> Self {
        Self {
            model: std::env::var("MIRA_MODEL").unwrap_or_else(|_| "gpt-5".to_string()),
            verbosity: std::env::var("MIRA_VERBOSITY").unwrap_or_else(|_| "medium".to_string()),
            reasoning_effort: std::env::var("MIRA_REASONING_EFFORT")
                .unwrap_or_else(|_| "medium".to_string()),
            max_output_tokens: std::env::var("MIRA_MAX_OUTPUT_TOKENS")
                .ok().and_then(|s| s.parse().ok()).unwrap_or(128000),
            history_message_cap: std::env::var("MIRA_HISTORY_MESSAGE_CAP")
                .ok().and_then(|s| s.parse().ok()).unwrap_or(24),
            history_token_limit: std::env::var("MIRA_HISTORY_TOKEN_LIMIT")
                .ok().and_then(|s| s.parse().ok()).unwrap_or(8000),
            max_retrieval_tokens: std::env::var("MIRA_MAX_RETRIEVAL_TOKENS")
                .ok().and_then(|s| s.parse().ok()).unwrap_or(2000),
            max_vector_search_results: std::env::var("MIRA_MAX_VECTOR_RESULTS")
                .ok().and_then(|s| s.parse().ok()).unwrap_or(5),
            enable_vector_search: std::env::var("MIRA_ENABLE_VECTOR_SEARCH")
                .unwrap_or_else(|_| "true".to_string()).parse::<bool>().unwrap_or(true),
            enable_web_search: std::env::var("MIRA_ENABLE_WEB_SEARCH")
                .unwrap_or_else(|_| "false".to_string()).parse::<bool>().unwrap_or(false),
            enable_code_interpreter: std::env::var("MIRA_ENABLE_CODE_INTERPRETER")
                .unwrap_or_else(|_| "false".to_string()).parse::<bool>().unwrap_or(false),
            enable_debug_logging: std::env::var("MIRA_DEBUG_LOGGING")
                .unwrap_or_else(|_| "false".to_string()).parse::<bool>().unwrap_or(false),
            enable_summarization: std::env::var("MIRA_ENABLE_SUMMARIZATION")
                .unwrap_or_else(|_| "true".to_string()).parse::<bool>().unwrap_or(true),
            summary_chunk_size: std::env::var("MIRA_SUMMARY_CHUNK_SIZE")
                .ok().and_then(|s| s.parse().ok()).unwrap_or(6),
            summary_token_limit: std::env::var("MIRA_SUMMARY_TOKEN_LIMIT")
                .ok().and_then(|s| s.parse().ok()).unwrap_or(2000),
            summary_output_tokens: std::env::var("MIRA_SUMMARY_OUTPUT_TOKENS")
                .ok().and_then(|s| s.parse().ok()).unwrap_or(512),
        }
    }
}

/// Main chat service orchestrator
pub struct ChatService {
    client: Arc<OpenAIClient>,
    threads: Arc<ThreadManager>,
    memory_service: Arc<MemoryService>,
    _vector_store_manager: Arc<VectorStoreManager>, // Keep field for struct completeness
    summarization_service: Arc<SummarizationService>,
    persona: PersonaOverlay,
    config: Arc<ChatConfig>,
}

impl ChatService {
    pub fn new(
        client: Arc<OpenAIClient>,
        threads: Arc<ThreadManager>,
        memory_service: Arc<MemoryService>,
        vector_store_manager: Arc<VectorStoreManager>,
        persona: PersonaOverlay,
    ) -> Self {
        let config = Arc::new(ChatConfig::from_env());

        let summarization_service = Arc::new(SummarizationService::new(
            threads.clone(),
            memory_service.clone(),
            client.clone(),
            config.clone(),
        ));

        Self {
            client,
            threads,
            memory_service,
            _vector_store_manager: vector_store_manager,
            summarization_service,
            persona,
            config,
        }
    }

    #[instrument(skip(self, _return_structured))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
        _return_structured: bool,
    ) -> Result<ChatResponse> {
        let start_time = Instant::now();
        info!("💬 Starting chat for session: {}", session_id);

        self.memory_service
            .save_user_message(session_id, user_text, project_id)
            .await?;

        self.threads.add_message(session_id, ResponseMessage {
            role: "user".to_string(),
            content: Some(user_text.to_string()),
            name: None,
            function_call: None,
            tool_calls: None,
        }).await?;

        if self.config.enable_summarization {
            debug!("Checking for summarization trigger");
            self.summarization_service
                .summarize_if_needed(session_id)
                .await?;
        }

        let context = self.build_recall_context(session_id, user_text, project_id).await?;

        let metadata = get_metadata(
            &self.client,
            user_text,
            &self.persona,
            &context,
        ).await?;

        let mut content_stream = get_content_stream(
            &self.client,
            user_text,
            &self.persona,
            &context,
            &metadata,
        ).await?;

        let mut full_content = String::new();
        while let Some(event) = content_stream.next().await {
            if let Ok(StreamEvent::Delta(chunk)) = event {
                full_content.push_str(&chunk);
            }
        }

        let complete_output = if metadata.output.is_empty() {
            full_content
        } else {
            format!("{}\n\n{}", metadata.output, full_content)
        };
        
        // Construct the final response using cloned data
        let response = self.construct_chat_response(complete_output, metadata);

        self.threads.add_message(session_id, ResponseMessage {
            role: "assistant".to_string(),
            content: Some(response.output.clone()),
            name: None,
            function_call: None,
            tool_calls: None,
        }).await?;

        self.memory_service.save_assistant_response(
            session_id,
            &response,
        ).await?;

        let elapsed = start_time.elapsed();
        info!("✅ Chat response generated in {:?}", elapsed);

        Ok(response)
    }

    async fn build_recall_context(
        &self,
        session_id: &str,
        user_text: &str,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        let recent_messages = self.memory_service
            .get_recent_context(session_id, self.config.history_message_cap)
            .await?;

        let embedding = self.client.get_embedding(user_text).await?;
        let similar_memories = self.memory_service
            .search_similar(session_id, &embedding, 3)
            .await?;

        Ok(RecallContext {
            recent: recent_messages,
            semantic: similar_memories,
        })
    }

    fn construct_chat_response(&self, output: String, metadata: ResponseMetadata) -> ChatResponse {
        ChatResponse {
            output,
            persona: self.persona.name().to_string(),
            mood: metadata.mood,
            salience: metadata.salience,
            summary: metadata.summary,
            memory_type: metadata.memory_type,
            tags: metadata.tags,
            intent: metadata.intent,
            monologue: metadata.monologue,
            reasoning_summary: metadata.reasoning_summary,
        }
    }
}
