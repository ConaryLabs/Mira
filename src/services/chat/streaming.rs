// src/services/chat/streaming.rs
// Manages the logic for handling and generating streaming chat responses.

use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info, error};

use crate::llm::client::OpenAIClient;
use crate::memory::recall::RecallContext;
use crate::persona::default::DEFAULT_PERSONA_PROMPT;

/// Handles the business logic for streaming chat responses.
pub struct StreamingHandler {
    llm_client: Arc<OpenAIClient>,
}

impl StreamingHandler {
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        Self { llm_client }
    }

    /// Generates a non-streaming response using the main client method.
    pub async fn generate_response(
        &self,
        user_text: &str,
        context: &RecallContext,
    ) -> Result<String> {
        debug!("Generating response for user text: \"{}\"", user_text.chars().take(80).collect::<String>());
        
        let system_prompt = self.build_system_prompt(context);
        
        let response = self.llm_client
            .generate_response(user_text, system_prompt.as_deref(), false)
            .await?;

        info!("Response generated successfully ({} chars)", response.content.len());
        Ok(response.content)
    }

    /// Builds a system prompt incorporating the provided context.
    fn build_system_prompt(&self, context: &RecallContext) -> Option<String> {
        let mut prompt = String::from(DEFAULT_PERSONA_PROMPT);
        
        if !context.recent.is_empty() {
            prompt.push_str("\n\n[You have access to recent conversation history. Use it naturally while staying as Mira - cursing, joking, being real.]");
        }
        
        if !context.semantic.is_empty() {
            prompt.push_str("\n\n[You have relevant context from previous conversations. Use it to be more helpful, but NEVER break character. Stay as Mira.]");
        }
        
        Some(prompt)
    }
}
