// src/memory/features/summarization/strategies/snapshot_summary.rs

use std::sync::Arc;
use anyhow::Result;
use tracing::info;
use crate::llm::provider::{LlmProvider, ChatMessage};
use crate::memory::core::types::MemoryEntry;

/// Handles on-demand snapshot summary operations
pub struct SnapshotSummaryStrategy {
    llm_provider: Arc<dyn LlmProvider>,
}

impl SnapshotSummaryStrategy {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }

    /// Creates comprehensive snapshot of current conversation state
    pub async fn create_summary(
        &self,
        session_id: &str,
        messages: &[MemoryEntry],
        _max_tokens: Option<usize>,
    ) -> Result<String> {
        if messages.is_empty() {
            return Err(anyhow::anyhow!("No messages to summarize"));
        }

        let content = self.build_content(messages)?;
        let prompt = self.build_prompt(&content);

        info!("Creating snapshot summary for session {} with {} messages", session_id, messages.len());
        
        // Use provider.chat() instead of summarize_conversation()
        let chat_messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        
        let response = self.llm_provider
            .chat(
                chat_messages,
                "You are a conversation summarizer. Create comprehensive, accurate snapshots.".to_string(),
                None, // No thinking for summaries
            )
            .await?;

        Ok(response.content)
    }

    fn build_content(&self, messages: &[MemoryEntry]) -> Result<String> {
        let mut content = String::new();
        
        for msg in messages.iter().rev() {
            // Include ALL messages for comprehensive snapshot
            content.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }
        
        Ok(content)
    }

    fn build_prompt(&self, content: &str) -> String {
        format!(
            "Create a comprehensive snapshot of this conversation. \
            Include all key topics, decisions, and important context:\n\n{}",
            content
        )
    }
}
