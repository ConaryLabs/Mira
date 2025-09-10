// Temporary wrapper after deleting duplicate streaming implementation
// TODO: Consolidate this in Phase 1 of cleanup
use std::sync::Arc;
use anyhow::Result;
use futures::StreamExt;
use crate::llm::client::OpenAIClient;
use crate::llm::streaming::{start_response_stream, StreamEvent};
use crate::memory::recall::RecallContext;

pub struct StreamingHandler {
    client: Arc<OpenAIClient>,
}

impl StreamingHandler {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self { client }
    }
    
    pub async fn generate_response(
        &self,
        user_text: &str,
        _context: &RecallContext,  // Context used elsewhere, not for system prompt
    ) -> Result<String> {
        // TODO: Build system prompt from context.recent and context.semantic
        // For now, just use a basic system prompt
        let system_prompt = Some("You are a helpful assistant.");
        
        // Use the actual streaming implementation from llm::streaming
        let mut stream = start_response_stream(
            &self.client,
            user_text,
            system_prompt,
            false  // structured_json - keeping it simple for now
        ).await?;
        
        let mut full_response = String::new();
        
        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::Delta(text) | StreamEvent::Text(text) => {
                    full_response.push_str(&text);
                }
                StreamEvent::Done { full_text, .. } => {
                    return Ok(full_text);
                }
                StreamEvent::Error(e) => {
                    return Err(anyhow::anyhow!("Streaming error: {}", e));
                }
            }
        }
        
        Ok(full_response)
    }
}
