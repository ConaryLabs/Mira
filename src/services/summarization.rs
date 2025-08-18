// src/services/summarization.rs

use std::sync::Arc;
use anyhow::{anyhow, Result};

use crate::llm::client::OpenAIClient;
use crate::services::chat::{ChatConfig, ChatResponse};
use crate::services::memory::MemoryService;
use crate::memory::traits::MemoryStore;

/// Summarizes recent conversation and persists a compact summary as a normal
/// assistant response (tags=["summary"], memory_type="context").
///
/// Wiring options:
/// - Minimal: call `new(openai, config)` and later call `attach_stores(...)`.
/// - Fully-wired: call `new_with_stores(openai, config, sqlite_store, memory_service)`.
pub struct SummarizationService {
    openai_client: Arc<OpenAIClient>,
    config: Arc<ChatConfig>,

    // Optional until attached; enables summarize_if_needed to operate.
    sqlite_store: Option<Arc<dyn MemoryStore + Send + Sync>>,
    memory_service: Option<Arc<MemoryService>>,
}

impl SummarizationService {
    pub fn new(openai_client: Arc<OpenAIClient>, config: Arc<ChatConfig>) -> Self {
        Self {
            openai_client,
            config,
            sqlite_store: None,
            memory_service: None,
        }
    }

    pub fn new_with_stores(
        openai_client: Arc<OpenAIClient>,
        config: Arc<ChatConfig>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        memory_service: Arc<MemoryService>,
    ) -> Self {
        Self {
            openai_client,
            config,
            sqlite_store: Some(sqlite_store),
            memory_service: Some(memory_service),
        }
    }

    /// If you constructed with `new(...)`, call this once during bootstrapping.
    pub fn attach_stores(
        &mut self,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        memory_service: Arc<MemoryService>,
    ) {
        self.sqlite_store = Some(sqlite_store);
        self.memory_service = Some(memory_service);
    }

    /// Summarize recent messages and persist the result as a ChatResponse.
    /// If stores haven’t been attached, this becomes a safe no-op (returns Ok(())).
    pub async fn summarize_if_needed(&self, session_id: &str) -> Result<()> {
        let sqlite = match &self.sqlite_store {
            Some(s) => s.clone(),
            None => return Ok(()), // not wired yet; skip without failing the pipeline
        };
        let mem = match &self.memory_service {
            Some(m) => m.clone(),
            None => return Ok(()),
        };

        // 1) Load a window of recent messages (use history_message_cap as guide).
        let cap = self.config.history_message_cap.max(8);
        let take = cap.saturating_mul(2);
        let recent = sqlite.load_recent(session_id, take).await?;
        if recent.is_empty() {
            return Ok(());
        }

        // 2) Build a clean prompt for summarization (order preserved).
        let mut prompt = String::from(
            "Summarize the following recent conversation for fast recall later.\n\
             Keep it faithful, concise, and useful for context stitching.\n\n",
        );
        for msg in &recent {
            // Expecting each entry to have role + content.
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        // 3) Ask the model for a compact summary within our configured token budget.
        let token_limit = self.config.max_output_tokens.min(1024);
        let summary = self
            .openai_client
            .summarize_conversation(&prompt, token_limit)
            .await?
            .trim()
            .to_string();

        if summary.is_empty() {
            // Don’t save an empty summary.
            return Ok(());
        }

        // 4) Persist as an assistant response so it’s retrievable like everything else.
        let response = ChatResponse {
            output: String::new(),            // not user-facing body; summary lives below
            persona: Some("mira".to_string()),
            mood: "neutral".to_string(),
            salience: 2,                      // low default; tune if desired
            summary,                          // the compacted conversation summary
            memory_type: "context".to_string(),
            tags: vec!["summary".to_string()],
            intent: Some("summarize".to_string()),
            monologue: None,
            reasoning_summary: None,
        };

        mem.save_assistant_response(session_id, &response).await?;
        Ok(())
    }

    /// Direct utility: summarize arbitrary text without touching storage.
    pub async fn summarize_text(&self, text: &str) -> Result<String> {
        if text.trim().is_empty() {
            return Err(anyhow!("summarize_text: empty input"));
        }
        let token_limit = self.config.max_output_tokens.min(1024);
        let out = self
            .openai_client
            .summarize_conversation(text, token_limit)
            .await?;
        Ok(out.trim().to_string())
    }
}
