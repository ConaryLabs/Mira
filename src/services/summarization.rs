// src/services/summarization.rs
use std::sync::Arc;
use anyhow::{anyhow, Result};
use tracing::{debug, info};

use crate::llm::client::OpenAIClient;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::memory::core::traits::MemoryStore;
use crate::services::chat::{ChatConfig, ChatResponse};
use crate::memory::MemoryService;
use crate::config::CONFIG;

pub struct SummarizationService {
    openai_client: Arc<OpenAIClient>,
    config: Arc<ChatConfig>,
    sqlite_store: Option<Arc<SqliteMemoryStore>>,
    memory_service: Option<Arc<MemoryService>>,
}

impl SummarizationService {
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

    /// Check recent conversation length & trigger legacy summarization if needed
    pub async fn summarize_if_needed(&self, session_id: &str) -> Result<()> {
        // Skip legacy summarization if rolling summaries are enabled
        if CONFIG.is_robust_memory_enabled() && 
           (CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100) {
            debug!("Skipping legacy summarization - rolling summaries are enabled");
            return Ok(());
        }

        if let Some(ref sqlite_store) = self.sqlite_store {
            let cap = self.config.history_message_cap().max(8);
            let recent = sqlite_store.load_recent(session_id, cap).await?;
            if recent.len() < cap / 2 {
                return Ok(());
            }
            self.summarize_recent(session_id).await
        } else {
            Ok(())
        }
    }

    /// Actually summarize recent messages and persist the summary
    pub async fn summarize_recent(&self, session_id: &str) -> Result<()> {
        let sqlite = self.sqlite_store
            .as_ref()
            .ok_or_else(|| anyhow!("No SQLite store available for summarization"))?;
        let mem = self.memory_service
            .as_ref()
            .ok_or_else(|| anyhow!("No memory service available for summarization"))?;

        let cap = self.config.history_message_cap().max(8);
        let take = cap.saturating_mul(2);
        let recent = sqlite.load_recent(session_id, take).await?;
        if recent.is_empty() {
            return Ok(());
        }

        let mut prompt = String::from(
            "Summarize the following recent conversation for fast recall later.\n\
             Keep it faithful, concise, and useful for context stitching.\n\n",
        );
        for msg in &recent {
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        let token_limit = self.config.max_output_tokens().min(1024);
        let summary = self
            .openai_client
            .summarize_conversation(&prompt, token_limit)
            .await?
            .trim()
            .to_string();

        if summary.is_empty() {
            return Ok(());
        }

        let response = ChatResponse {
            output: String::new(),
            persona: "mira".to_string(),
            mood: "neutral".to_string(),
            salience: 2,
            summary,
            memory_type: "context".to_string(),
            tags: vec!["summary".to_string()],
            intent: Some("summarize".to_string()),
            monologue: None,
            reasoning_summary: None,
        };

        mem.save_assistant_response(session_id, &response).await?;
        Ok(())
    }
}
