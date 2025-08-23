// src/services/summarization.rs
use std::sync::Arc;
use anyhow::{anyhow, Result};

use crate::llm::client::OpenAIClient;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::services::chat::{ChatConfig, ChatResponse};
use crate::services::memory::MemoryService;

pub struct SummarizationService {
    openai_client: Arc<OpenAIClient>,
    config: Arc<ChatConfig>,
    sqlite_store: Option<Arc<SqliteMemoryStore>>,
    memory_service: Option<Arc<MemoryService>>,
}

impl SummarizationService {
    /// Create a summarizer for direct summarization (text-only)
    pub fn new(openai_client: Arc<OpenAIClient>, config: Arc<ChatConfig>) -> Self {
        // Placeholder: we don't have stores here, so summarize_if_needed() is a no-op.
        Self {
            openai_client,
            config,
            sqlite_store: None,
            memory_service: None,
        }
    }

    /// Create a summarizer that can actually read & write memory (used for ChatService).
    pub fn new_with_stores(
        openai_client: Arc<OpenAIClient>,
        config: Arc<ChatConfig>,
        sqlite_store: Arc<SqliteMemoryStore>,
        memory_service: Arc<MemoryService>,
    ) -> Self {
        Self {
            openai_client,
            config,
            sqlite_store: Some(sqlite_store),
            memory_service: Some(memory_service),
        }
    }

    /// Check recent conversation length & trigger summarization if needed.
    pub async fn summarize_if_needed(&self, session_id: &str) -> Result<()> {
        // We only summarize if we have real stores; if not, return early.
        if let Some(ref sqlite_store) = self.sqlite_store {
            let cap = self.config.history_message_cap().max(8);
            let recent = sqlite_store.load_recent(session_id, cap).await?;
            if recent.len() < cap / 2 {
                // Too few messages to bother summarizing.
                return Ok(());
            }
            self.summarize_recent(session_id).await
        } else {
            // No stores available, skip summarization
            Ok(())
        }
    }

    /// Actually summarize recent messages and persist the summary.
    pub async fn summarize_recent(&self, session_id: &str) -> Result<()> {
        let sqlite = self.sqlite_store
            .as_ref()
            .ok_or_else(|| anyhow!("No SQLite store available for summarization"))?;
        let mem = self.memory_service
            .as_ref()
            .ok_or_else(|| anyhow!("No memory service available for summarization"))?;

        // 1) Load recent messages (more than config cap to ensure we catch the tail).
        let cap = self.config.history_message_cap().max(8);
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
        let token_limit = self.config.max_output_tokens().min(1024);
        let summary = self
            .openai_client
            .summarize_conversation(&prompt, token_limit)
            .await?
            .trim()
            .to_string();

        if summary.is_empty() {
            // Don't save an empty summary.
            return Ok(());
        }

        // 4) Persist as an assistant response so it's retrievable like everything else.
        let response = ChatResponse {
            output: String::new(),            // not user-facing body; summary lives below
            persona: "mira".to_string(),      // FIXED: was Some("mira".to_string())
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
        let token_limit = self.config.max_output_tokens().min(1024);
        let out = self
            .openai_client
            .summarize_conversation(text, token_limit)
            .await?;
        Ok(out.trim().to_string())
    }
}
