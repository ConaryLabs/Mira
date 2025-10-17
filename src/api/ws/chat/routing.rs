// src/api/ws/chat/routing.rs
// LLM-based message routing - decides if message should route to OperationEngine

use crate::llm::provider::gpt5::Gpt5Provider;
use crate::llm::provider::{Message, LlmProvider};
use anyhow::Result;

pub struct MessageRouter {
    gpt5: Gpt5Provider,
}

impl MessageRouter {
    pub fn new(gpt5: Gpt5Provider) -> Self {
        Self { gpt5 }
    }
    
    /// Use LLM to determine if this message should route to OperationEngine
    /// Returns true if the message is a coding/implementation request
    pub async fn should_route_to_operation(&self, message: &str) -> Result<bool> {
        let system = r#"You are a routing classifier. Determine if the user's message is requesting:
- Code generation
- Code modification
- Creating files/projects
- Technical implementation
- Building or creating something with code

Respond with ONLY "yes" or "no"."#;
        
        let messages = vec![Message::user(message.to_string())];
        
        let response = self.gpt5.chat(messages, system.to_string()).await?;
        
        Ok(response.content.trim().to_lowercase() == "yes")
    }
}
