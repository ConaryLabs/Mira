use std::sync::Arc;
use anyhow::Result;
use tracing::info;
use crate::llm::client::OpenAIClient;
use crate::memory::core::types::MemoryEntry;

/// Handles on-demand snapshot summary operations
pub struct SnapshotSummaryStrategy {
    llm_client: Arc<OpenAIClient>,
}

impl SnapshotSummaryStrategy {
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        Self { llm_client }
    }

    /// Creates comprehensive snapshot of current conversation state
    pub async fn create_summary(
        &self,
        session_id: &str,
        messages: &[MemoryEntry],
        max_tokens: Option<usize>,
    ) -> Result<String> {
        if messages.is_empty() {
            return Err(anyhow::anyhow!("No messages to summarize"));
        }

        let content = self.build_content(messages)?;
        let prompt = self.build_prompt(&content);
        let token_limit = max_tokens.unwrap_or(1000);

        info!("Creating snapshot summary for session {} with {} messages", session_id, messages.len());
        
        let summary = self.llm_client
            .summarize_conversation(&prompt, token_limit)
            .await?;

        Ok(summary)
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
