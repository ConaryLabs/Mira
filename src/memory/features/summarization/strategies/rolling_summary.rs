// src/memory/features/summarization/strategies/rolling_summary.rs

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug};
use crate::llm::provider::{LlmProvider, ChatMessage};
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::memory_types::SummaryType;

/// Handles all rolling summary operations (10-message and 100-message windows)
pub struct RollingSummaryStrategy {
    llm_provider: Arc<dyn LlmProvider>,
}

impl RollingSummaryStrategy {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }

    /// Creates rolling summary for specified window size
    pub async fn create_summary(
        &self,
        session_id: &str,
        messages: &[MemoryEntry],
        window_size: usize,
    ) -> Result<String> {
        if messages.len() < window_size / 2 {
            return Err(anyhow::anyhow!("Insufficient messages for {}-window summary", window_size));
        }

        let content = self.build_content(messages)?;
        let prompt = self.build_prompt(&content, window_size);

        info!("Creating {}-message rolling summary for session {}", window_size, session_id);
        
        // Use provider.chat() instead of summarize_conversation()
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        
        let response = self.llm_provider
            .chat(
                messages,
                "You are a conversation summarizer. Create concise, accurate summaries.".to_string(),
                None, // No thinking for summaries
            )
            .await?;

        Ok(response.content)
    }

    /// Determines if rolling summary should be created based on message count
    pub fn should_create(&self, message_count: usize) -> Option<SummaryType> {
        // Every 10 messages - lightweight rolling summary
        if message_count > 0 && message_count % 10 == 0 {
            return Some(SummaryType::Rolling10);
        }
        
        // Every 100 messages - comprehensive mega-summary  
        if message_count > 0 && message_count % 100 == 0 {
            return Some(SummaryType::Rolling100);
        }

        None
    }

    fn build_content(&self, messages: &[MemoryEntry]) -> Result<String> {
        let mut content = String::new();
        let mut included_count = 0;
        
        for msg in messages.iter().rev() {
            // Skip existing summaries to avoid recursive summarization
            if let Some(ref tags) = msg.tags {
                if tags.iter().any(|t| t.contains("summary")) {
                    debug!("Skipping existing summary in content building");
                    continue;
                }
            }
            
            content.push_str(&format!("{}: {}\n", msg.role, msg.content));
            included_count += 1;
        }
        
        debug!("Built rolling summary content from {} messages", included_count);
        Ok(content)
    }

    fn build_prompt(&self, content: &str, window_size: usize) -> String {
        match window_size {
            100 => format!(
                "Create a comprehensive mega-summary of the last {} messages. \
                Focus on key themes, important decisions, and critical information. \
                Preserve context and maintain continuity:\n\n{}",
                window_size, content
            ),
            10 => format!(
                "Create a concise rolling summary of the last {} messages. \
                Capture key points and maintain conversation context:\n\n{}",
                window_size, content
            ),
            _ => format!(
                "Summarize the last {} messages, preserving important details:\n\n{}",
                window_size, content
            ),
        }
    }
}
