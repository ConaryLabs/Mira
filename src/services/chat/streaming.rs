// src/services/chat/streaming.rs
// Extracted Streaming Logic from chat.rs
// Handles response streaming and content generation

use std::sync::Arc;
use anyhow::Result;
use futures::StreamExt;
use tracing::{info, warn, debug};

use crate::llm::client::OpenAIClient;
use crate::llm::streaming::{StreamEvent, start_response_stream};
use crate::memory::recall::RecallContext;
use crate::api::error::IntoApiError;

/// Streaming handler for chat responses
pub struct StreamingHandler {
    llm_client: Arc<OpenAIClient>,
}

impl StreamingHandler {
    /// Create new streaming handler
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        Self { llm_client }
    }

    /// Generate response using streaming
    pub async fn generate_response(
        &self,
        user_text: &str,
        context: &RecallContext,
    ) -> Result<String> {
        info!("🎯 Generating streaming response");

        // Build system prompt with context
        let system_prompt = self.build_system_prompt(context);
        
        debug!("System prompt built with {} recent and {} semantic matches",
               context.recent.len(), context.semantic.len());

        // Start streaming response
        let mut stream = start_response_stream(
            &self.llm_client,
            user_text,
            system_prompt.as_deref(),
            false, // structured_json = false for regular chat
        )
        .await
        .into_api_error("Failed to start response stream")?;

        // Collect streamed content
        let mut full_content = String::new();
        let mut chunk_count = 0;

        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::Delta(chunk)) => {
                    full_content.push_str(&chunk);
                    chunk_count += 1;
                    
                    // Debug log every 10 chunks to avoid spam
                    if chunk_count % 10 == 0 {
                        debug!("Received {} chunks, total length: {}", chunk_count, full_content.len());
                    }
                }
                Ok(StreamEvent::Done { full_text: _, raw: _ }) => {
                    info!("✅ Stream completed with {} chunks", chunk_count);
                    break;
                }
                Ok(StreamEvent::Error(e)) => {
                    warn!("⚠️ Stream error: {}", e);
                    return Err(anyhow::anyhow!("Stream error: {}", e));
                }
                Err(e) => {
                    warn!("⚠️ Stream parsing error: {}", e);
                    return Err(anyhow::anyhow!("Stream parsing error: {}", e));
                }
            }
        }

        if full_content.is_empty() {
            warn!("⚠️ Empty response from stream");
            return Ok("I'm sorry, I couldn't generate a response. Please try again.".to_string());
        }

        info!("✅ Generated response: {} characters", full_content.len());
        Ok(full_content)
    }

    /// Generate response with custom system prompt
    pub async fn generate_response_with_prompt(
        &self,
        user_text: &str,
        system_prompt: Option<&str>,
    ) -> Result<String> {
        info!("🎯 Generating response with custom prompt");

        let mut stream = start_response_stream(
            &self.llm_client,
            user_text,
            system_prompt,
            false,
        )
        .await
        .into_api_error("Failed to start custom response stream")?;

        let mut full_content = String::new();
        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::Delta(chunk)) => {
                    full_content.push_str(&chunk);
                }
                Ok(StreamEvent::Done { full_text: _, raw: _ }) => break,
                Ok(StreamEvent::Error(e)) => {
                    return Err(anyhow::anyhow!("Stream error: {}", e));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Stream parsing error: {}", e));
                }
            }
        }

        Ok(full_content)
    }

    /// Generate structured response (JSON mode)
    pub async fn generate_structured_response(
        &self,
        user_text: &str,
        system_prompt: Option<&str>,
    ) -> Result<String> {
        info!("🎯 Generating structured JSON response");

        let mut stream = start_response_stream(
            &self.llm_client,
            user_text,
            system_prompt,
            true, // structured_json = true
        )
        .await
        .into_api_error("Failed to start structured response stream")?;

        let mut full_content = String::new();
        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::Delta(chunk)) => {
                    full_content.push_str(&chunk);
                }
                Ok(StreamEvent::Done { full_text: _, raw: _ }) => break,
                Ok(StreamEvent::Error(e)) => {
                    return Err(anyhow::anyhow!("Structured stream error: {}", e));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Structured stream parsing error: {}", e));
                }
            }
        }

        // Validate JSON if we expect structured output
        if !full_content.trim().is_empty() {
            if let Err(e) = serde_json::from_str::<serde_json::Value>(&full_content) {
                warn!("⚠️ Generated content is not valid JSON: {}", e);
                return Err(anyhow::anyhow!("Invalid JSON response: {}", e));
            }
        }

        Ok(full_content)
    }

    /// Build system prompt incorporating conversation context
    fn build_system_prompt(&self, context: &RecallContext) -> Option<String> {
        let mut prompt = String::from("You are Mira, a helpful AI assistant. Be concise and provide useful responses.");

        // Add context information if available
        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\nConversation context:");
            
            if !context.recent.is_empty() {
                prompt.push_str(&format!("\nRecent messages ({}): Reference when relevant for continuity.", context.recent.len()));
            }
            
            if !context.semantic.is_empty() {
                prompt.push_str(&format!("\nSemantic matches ({}): Use for additional context when helpful.", context.semantic.len()));
            }
            
            prompt.push_str("\n\nUse this context naturally without explicitly mentioning it unless directly relevant.");
        }

        Some(prompt)
    }

    /// Build system prompt with custom persona
    pub fn build_system_prompt_with_persona(
        &self,
        base_persona: &str,
        context: &RecallContext,
        additional_instructions: Option<&str>,
    ) -> String {
        let mut prompt = format!("You are {}.", base_persona);

        // Add additional instructions if provided
        if let Some(instructions) = additional_instructions {
            prompt.push_str(&format!(" {}", instructions));
        }

        // Add context information
        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\nContext available:");
            
            if !context.recent.is_empty() {
                prompt.push_str(&format!(
                    "\n- Recent conversation: {} messages",
                    context.recent.len()
                ));
            }
            
            if !context.semantic.is_empty() {
                prompt.push_str(&format!(
                    "\n- Semantic context: {} relevant items",
                    context.semantic.len()
                ));
            }
        }

        prompt
    }

    /// Test streaming connection
    pub async fn test_streaming(&self) -> Result<bool> {
        info!("🔧 Testing streaming connection");

        let test_message = "Hello, this is a connection test.";
        let system_prompt = "You are a test assistant. Respond with exactly 'Test successful'.";

        match self.generate_response_with_prompt(test_message, Some(system_prompt)).await {
            Ok(response) => {
                let success = response.contains("Test successful") || response.len() > 0;
                info!("✅ Streaming test completed: {}", if success { "SUCCESS" } else { "PARTIAL" });
                Ok(success)
            }
            Err(e) => {
                warn!("❌ Streaming test failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Get streaming statistics
    pub fn get_streaming_stats(&self) -> StreamingStats {
        // In a real implementation, we'd track these metrics
        StreamingStats {
            total_streams: 0,
            successful_streams: 0,
            failed_streams: 0,
            average_response_time: 0.0,
            total_tokens_streamed: 0,
        }
    }
}

/// Streaming performance statistics
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
            return 0.0;
        }
        self.successful_streams as f64 / self.total_streams as f64 * 100.0
    }

    pub fn failure_rate(&self) -> f64 {
        if self.total_streams == 0 {
            return 0.0;
        }
        self.failed_streams as f64 / self.total_streams as f64 * 100.0
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
    use crate::memory::recall::RecallContext;

    #[test]
    fn test_system_prompt_building() {
        let handler = StreamingHandler::new(Arc::new(
            // This would need a proper mock in real tests
            todo!("Mock OpenAI client for testing")
        ));

        let context = RecallContext {
            recent: vec![/* mock recent messages */],
            semantic: vec![/* mock semantic matches */],
        };

        let prompt = handler.build_system_prompt(&context);
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("Mira"));
    }

    #[test]
    fn test_persona_prompt_building() {
        let handler = StreamingHandler::new(Arc::new(
            todo!("Mock OpenAI client for testing")
        ));

        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };

        let prompt = handler.build_system_prompt_with_persona(
            "Mira, a helpful assistant",
            &context,
            Some("Be concise in your responses."),
        );

        assert!(prompt.contains("Mira"));
        assert!(prompt.contains("concise"));
    }

    #[test]
    fn test_streaming_stats() {
        let stats = StreamingStats {
            total_streams: 100,
            successful_streams: 95,
            failed_streams: 5,
            average_response_time: 1.5,
            total_tokens_streamed: 50000,
        };

        assert_eq!(stats.success_rate(), 95.0);
        assert_eq!(stats.failure_rate(), 5.0);
        assert_eq!(stats.average_tokens_per_stream(), 526.3157894736842);
    }
}
