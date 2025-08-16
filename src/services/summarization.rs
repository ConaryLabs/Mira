use crate::{
    llm::{
        client::OpenAIClient,
        responses::thread::{ResponseMessage, ThreadManager},
    },
    services::{chat::ChatConfig, memory::MemoryService},
};
use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// A service dedicated to summarizing conversation history when it exceeds token or message limits.
pub struct SummarizationService {
    pub thread_manager: Arc<ThreadManager>,
    pub memory_service: Arc<MemoryService>,
    pub openai_client: Arc<OpenAIClient>,
    pub config: Arc<ChatConfig>,
}

impl SummarizationService {
    /// Creates a new instance of the SummarizationService.
    pub fn new(
        thread_manager: Arc<ThreadManager>,
        memory_service: Arc<MemoryService>,
        openai_client: Arc<OpenAIClient>,
        config: Arc<ChatConfig>,
    ) -> Self {
        Self {
            thread_manager,
            memory_service,
            openai_client,
            config,
        }
    }

    /// Checks if the conversation history for a given session exceeds the configured limits
    /// and, if so, triggers the summarization process.
    pub async fn summarize_if_needed(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.thread_manager.sessions.write().await;
        
        if let Some(session) = sessions.get_mut(session_id) {
            let message_count = session.messages.len();
            let token_count = session.total_tokens;

            let message_cap = self.config.history_message_cap;
            let token_limit = self.config.history_token_limit;

            // 1. Check if limits are exceeded. If not, return immediately. [cite: 80]
            if message_count <= message_cap && token_count <= token_limit {
                return Ok(());
            }

            info!(
                "Summarization triggered for session {}: messages {}/{}, tokens {}/{}",
                session_id, message_count, message_cap, token_count, token_limit
            );

            // 2. Identify messages to summarize. [cite: 82]
            // We'll take a chunk from the beginning of the history.
            let chunk_size = std::cmp::min(self.config.summary_chunk_size, message_count);
            if chunk_size == 0 {
                return Ok(());
            }
            
            // Temporarily take ownership of messages to summarize
            let messages_to_summarize: Vec<ResponseMessage> = session.messages.drain(0..chunk_size).collect();
            let original_message_count = messages_to_summarize.len();

            // 3. Convert them to a single text prompt. [cite: 53-57, 83]
            let prompt_text = self.build_summarization_prompt(&messages_to_summarize);

            // Call the OpenAI client to get the summary. [cite: 83]
            match self.openai_client.summarize_conversation(&prompt_text, self.config.summary_output_tokens).await {
                Ok(summary_text) => {
                    info!("Successfully generated summary for {} messages.", original_message_count);
                    
                    // 4. Create and insert a summary message into the thread at the front. [cite: 63, 84]
                    let summary_message = ResponseMessage {
                        role: "system".to_string(),
                        content: Some(summary_text.clone()),
                        name: None,
                        function_call: None,
                        tool_calls: None,
                    };
                    session.messages.push_front(summary_message);

                    // 5. Create a MemoryEntry for the summary and save it. [cite: 65, 85]
                    if let Err(e) = self.memory_service.save_summary(session_id, &summary_text, original_message_count).await {
                        warn!("Failed to save summary to memory service: {}", e);
                    }
                    
                    // 6. Recalculate total tokens for accuracy
                    session.total_tokens = session.messages.iter().map(|m| {
                        m.content.as_deref().unwrap_or("").len() / 4 + 10
                    }).sum();

                    debug!("Session {} post-summary: {} messages, ~{} tokens", session_id, session.messages.len(), session.total_tokens);

                }
                Err(e) => {
                    warn!("Summarization failed: {}. Re-inserting original messages.", e);
                    // If summarization fails, put the messages back to avoid data loss.
                    for msg in messages_to_summarize.into_iter().rev() {
                        session.messages.push_front(msg);
                    }
                }
            }
        }
        Ok(())
    }

    /// Builds the prompt for the summarization LLM call.
    fn build_summarization_prompt(&self, messages: &[ResponseMessage]) -> String {
        let mut prompt = String::from("Summarise the following conversation excerpts. Retain important facts, promises, feelings and humour. Include a brief description of the emotional tone and any relevant tags. Do not invent details.\n\n");

        for message in messages {
            let content = message.content.as_deref().unwrap_or("").trim();
            if !content.is_empty() {
                let line = format!("{}: {}\n", message.role, content);
                prompt.push_str(&line);
            }
        }
        prompt
    }
}
