// src/services/chat/streaming.rs
// Manages the logic for handling and generating streaming chat responses.
// FIXED: Now uses Mira's real personality instead of generic assistant bullshit

use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info, error};

use crate::llm::client::OpenAIClient;
use crate::memory::recall::RecallContext;
use crate::persona::default::DEFAULT_PERSONA_PROMPT; // Import Mira's REAL personality!

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

    /// Generates a response with a custom system prompt.
    pub async fn generate_response_with_prompt(
        &self,
        user_text: &str,
        system_prompt: Option<&str>,
    ) -> Result<String> {
        debug!("Generating response with custom system prompt.");
        
        let response = self.llm_client
            .generate_response(user_text, system_prompt, false)
            .await?;

        info!("Custom prompt response generated successfully ({} chars)", response.content.len());
        Ok(response.content)
    }

    /// Builds a system prompt incorporating the provided context.
    fn build_system_prompt(&self, context: &RecallContext) -> Option<String> {
        // ALWAYS use Mira's real personality, not generic assistant crap
        let mut prompt = String::from(DEFAULT_PERSONA_PROMPT);
        
        if !context.recent.is_empty() {
            prompt.push_str("\n\n[You have access to recent conversation history. Use it naturally while staying as Mira - cursing, joking, being real.]");
        }
        
        if !context.semantic.is_empty() {
            prompt.push_str("\n\n[You have relevant context from previous conversations. Use it to be more helpful, but NEVER break character. Stay as Mira.]");
        }
        
        Some(prompt)
    }

    /// Builds a system prompt that includes a specific persona and context.
    pub fn build_system_prompt_with_persona(
        &self,
        persona: &str,
        context: &RecallContext,
        additional_instructions: Option<&str>,
    ) -> String {
        // If they're trying to override Mira, at least keep her essence
        let mut prompt = if persona.to_lowercase().contains("mira") {
            // If it's still Mira, use the full personality
            String::from(DEFAULT_PERSONA_PROMPT)
        } else {
            // Even with a different persona name, inject Mira's personality traits
            format!("{}\n\nBut underneath it all, keep Mira's essence - be real, curse naturally, make jokes, don't be a bland assistant.", DEFAULT_PERSONA_PROMPT)
        };
        
        if let Some(instructions) = additional_instructions {
            prompt.push_str(&format!("\n\nAdditional context: {}", instructions));
        }
        
        if !context.recent.is_empty() {
            prompt.push_str("\n\n[You have access to recent conversation history.]");
        }
        
        prompt
    }

    /// Tests the connection to the streaming service.
    pub async fn test_streaming_connection(&self) -> Result<bool> {
        info!("Testing streaming connection...");
        let test_message = "Hello, this is a connection test.";
        let system_prompt = "You are a test assistant. Respond with exactly 'Test successful'.";

        match self.generate_response_with_prompt(test_message, Some(system_prompt)).await {
            Ok(response) => {
                let success = response.contains("Test successful");
                info!("Streaming test completed: {}", if success { "SUCCESS" } else { "PARTIAL SUCCESS" });
                Ok(success)
            }
            Err(e) => {
                error!("Streaming test failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Retrieves streaming performance statistics.
    pub fn get_streaming_stats(&self) -> StreamingStats {
        // In a real implementation, these metrics would be tracked.
        StreamingStats {
            total_streams: 0,
            successful_streams: 0,
            failed_streams: 0,
            average_response_time: 0.0,
            total_tokens_streamed: 0,
        }
    }
}

/// Contains performance statistics for streaming operations.
#[derive(Debug, Clone)]
pub struct StreamingStats {
    pub total_streams: u64,
    pub successful_streams: u64,
    pub failed_streams: u64,
    pub average_response_time: f64,
    pub total_tokens_streamed: u64,
}

impl StreamingStats {
    pub fn success_rate(&self) -> f64 {
        if self.total_streams == 0 {
            return 100.0;
        }
        (self.successful_streams as f64 / self.total_streams as f64) * 100.0
    }

    pub fn failure_rate(&self) -> f64 {
        if self.total_streams == 0 {
            return 0.0;
        }
        (self.failed_streams as f64 / self.total_streams as f64) * 100.0
    }

    pub fn average_tokens_per_stream(&self) -> f64 {
        if self.successful_streams == 0 {
            return 0.0;
        }
        self.total_tokens_streamed as f64 / self.successful_streams as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::client::{ClientConfig, OpenAIClient};
    use crate::llm::config::ModelConfig;
    use crate::memory::types::MemoryEntry;

    /// Creates a mock OpenAI client for testing purposes.
    fn create_mock_client() -> Arc<OpenAIClient> {
        let config = ClientConfig::from_model_config(ModelConfig {
            model: "gpt-mock".to_string(),
            verbosity: "low".to_string(),
            reasoning_effort: "low".to_string(),
            max_output_tokens: 100,
        }).unwrap();
        OpenAIClient::with_config(config).expect("Failed to create test client")
    }

    #[test]
    fn test_system_prompt_building_empty_context() {
        let mock_client = create_mock_client();
        let handler = StreamingHandler::new(mock_client);
        let context = RecallContext { recent: vec![], semantic: vec![] };
        let prompt = handler.build_system_prompt(&context).unwrap();
        assert!(prompt.contains("You are Mira"));
        assert!(prompt.contains("curse naturally")); // Check for real personality
        assert!(prompt.contains("dirty jokes")); // More real personality checks
        assert!(!prompt.contains("helpful AI assistant")); // NO GENERIC BULLSHIT
    }

    #[test]
    fn test_system_prompt_building_with_context() {
        let mock_client = create_mock_client();
        let handler = StreamingHandler::new(mock_client);
        let context = RecallContext {
            recent: vec![MemoryEntry::default()],
            semantic: vec![],
        };
        let prompt = handler.build_system_prompt(&context).unwrap();
        assert!(prompt.contains("recent conversation history"));
        assert!(prompt.contains("staying as Mira")); // Ensure she stays herself
        assert!(prompt.contains("cursing, joking, being real")); // Check personality preservation
    }

    #[test]
    fn test_persona_prompt_building() {
        let mock_client = create_mock_client();
        let handler = StreamingHandler::new(mock_client);
        let context = RecallContext { recent: vec![], semantic: vec![] };
        let prompt = handler.build_system_prompt_with_persona(
            "Test Persona",
            &context,
            Some("Be brief."),
        );
        // Even with different persona, Mira's essence should be there
        assert!(prompt.contains("Mira"));
        assert!(prompt.contains("Be brief."));
        assert!(prompt.contains("essence")); // Check that we keep Mira's essence
    }

    #[test]
    fn test_streaming_stats_calculations() {
        let stats = StreamingStats {
            total_streams: 100,
            successful_streams: 95,
            failed_streams: 5,
            average_response_time: 1.5,
            total_tokens_streamed: 50000,
        };
        assert!((stats.success_rate() - 95.0).abs() < f64::EPSILON);
        assert!((stats.failure_rate() - 5.0).abs() < f64::EPSILON);
        assert!((stats.average_tokens_per_stream() - 50000.0 / 95.0).abs() < f64::EPSILON);
    }
}
