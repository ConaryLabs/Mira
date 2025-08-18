// src/services/chat.rs
use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{info, instrument};
use futures::StreamExt;

use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::ThreadManager;
use crate::llm::responses::vector_store::VectorStoreManager;
use crate::llm::streaming::StreamEvent;
use crate::services::memory::MemoryService;
use crate::services::summarization::SummarizationService;
use crate::memory::recall::{RecallContext, build_context};
use crate::memory::traits::MemoryStore;
use crate::persona::PersonaOverlay;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: usize,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: Option<String>,
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
}

impl Default for ChatConfig {
    fn default() -> Self {
        ChatConfig {
            model: std::env::var("MIRA_MODEL").unwrap_or_else(|_| "gpt-5".to_string()),
            verbosity: std::env::var("MIRA_VERBOSITY").unwrap_or_else(|_| "medium".to_string()),
            reasoning_effort: std::env::var("MIRA_REASONING_EFFORT").unwrap_or_else(|_| "medium".to_string()),
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
                .unwrap_or_else(|_| "true".to_string())
                .parse::<bool>().unwrap_or(true),
            enable_web_search: std::env::var("MIRA_ENABLE_WEB_SEARCH")
                .unwrap_or_else(|_| "false".to_string())
                .parse::<bool>().unwrap_or(false),
            enable_code_interpreter: std::env::var("MIRA_ENABLE_CODE_INTERPRETER")
                .unwrap_or_else(|_| "false".to_string())
                .parse::<bool>().unwrap_or(false),
        }
    }
}

pub struct ChatService {
    pub client: Arc<OpenAIClient>,
    pub thread_mgr: Arc<ThreadManager>,
    pub vector_store: Arc<VectorStoreManager>,
    pub persona: PersonaOverlay,
    pub memory: Arc<MemoryService>,
    pub sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
    pub qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
    pub config: ChatConfig,
    pub summarizer: Arc<SummarizationService>,
}

impl ChatService {
    pub fn new(
        client: Arc<OpenAIClient>,
        thread_mgr: Arc<ThreadManager>,
        vector_store: Arc<VectorStoreManager>,
        persona: PersonaOverlay,
        memory: Arc<MemoryService>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
        summarizer: Arc<SummarizationService>,
        config: Option<ChatConfig>,
    ) -> Self {
        ChatService {
            client,
            thread_mgr,
            vector_store,
            persona,
            memory,
            sqlite_store,
            qdrant_store,
            summarizer,
            config: config.unwrap_or_default(),
        }
    }

    #[instrument(skip(self))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        let start = Instant::now();
        info!("chat(): session_id={}", session_id);

        // 1) Persist the user message (matches services/memory.rs signature)
        self.memory
            .save_user_message(session_id, user_text, project_id)
            .await?;

        // 2) Build recall context using both stores (recent + semantic)
        let embedding = self.client.get_embedding(user_text).await.ok();
        let context = build_context(
            session_id,
            embedding.as_deref(),
            self.config.history_message_cap,
            self.config.max_vector_search_results,
            self.sqlite_store.as_ref(),
            self.qdrant_store.as_ref(),
        )
        .await
        .unwrap_or_else(|_| RecallContext { recent: vec![], semantic: vec![] });

        // 3) Phase 1: metadata
        let metadata = crate::api::two_phase::get_metadata(
            &self.client,
            user_text,
            &self.persona,
            &context,
        )
        .await?;

        // 4) Phase 2: content (bind owned copies for lifetimes)
        let mood = metadata.mood.clone();
        let mut content_stream = crate::api::two_phase::get_content_stream(
            &self.client,
            user_text,
            &self.persona,
            &context,
            &mood,
            &metadata.intent,
        )
        .await?;

        let mut full_content = String::new();
        while let Some(event) = content_stream.next().await {
            if let Ok(StreamEvent::Delta(chunk)) = event {
                full_content.push_str(&chunk);
            }
        }

        let response = ChatResponse {
            output: full_content,
            persona: self.persona.to_string(),
            mood: metadata.mood,
            salience: metadata.salience, // usize in your struct
            summary: metadata.summary,   // String in your struct
            memory_type: if metadata.memory_type.is_empty() {
                "other".into()
            } else {
                metadata.memory_type
            },
            tags: metadata.tags,
            intent: metadata.intent,
            monologue: metadata.monologue,
            reasoning_summary: metadata.reasoning_summary,
        };

        // 6) Persist assistant response
        self.memory
            .save_assistant_response(session_id, &response)
            .await?;

        // 7) Summarize if needed (now wired for real work)
        self.summarizer.summarize_if_needed(session_id).await?;

        info!("chat() done in {:?}", start.elapsed());
        Ok(response)
    }

    /// Public helper kept for callers that want to build context directly.
    pub async fn build_recall_context(
        &self,
        session_id: &str,
        user_text: &str,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        let embedding = self.client.get_embedding(user_text).await.ok();

        let recent = self
            .sqlite_store
            .load_recent(session_id, self.config.history_message_cap)
            .await?;
        let semantic = if let Some(ref emb) = embedding {
            self.qdrant_store
                .semantic_search(session_id, emb, self.config.max_vector_search_results)
                .await?
        } else {
            Vec::new()
        };

        Ok(RecallContext { recent, semantic })
    }
}
