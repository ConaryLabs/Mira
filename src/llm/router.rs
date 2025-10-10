// src/llm/router.rs
// LLM router providing unified access to GPT-5 provider

use crate::llm::provider::{LlmProvider, Message, Response, ToolResponse, ToolContext};
use crate::llm::provider::gpt5::Gpt5Provider;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tracing::info;

pub struct LlmRouter {
    gpt5: Arc<Gpt5Provider>,
}

impl LlmRouter {
    pub fn new(gpt5: Arc<Gpt5Provider>) -> Self {
        Self { gpt5 }
    }
    
    pub fn get_provider(&self) -> Arc<dyn LlmProvider> {
        self.gpt5.clone() as Arc<dyn LlmProvider>
    }
    
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        system: String,
    ) -> Result<Response> {
        self.gpt5.chat(messages, system).await
    }
    
    pub async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        self.gpt5.chat_with_tools(messages, system, tools, context).await
    }
    
    /// Multi-turn tool calling with GPT-5
    pub async fn call_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        max_iterations: usize,
    ) -> Result<ToolResponse> {
        let mut current_messages = messages;
        let mut iteration = 0;
        let mut context: Option<ToolContext> = None;
        
        loop {
            iteration += 1;
            
            if iteration > max_iterations {
                info!("Maximum iteration limit reached");
                break;
            }
            
            let response = self.gpt5.chat_with_tools(
                current_messages.clone(),
                system.clone(),
                tools.clone(),
                context.clone(),
            ).await?;
            
            if response.function_calls.is_empty() {
                return Ok(response);
            }
            
            context = Some(ToolContext::Gpt5 {
                previous_response_id: response.id.clone(),
            });
            
            current_messages.push(Message {
                role: "assistant".to_string(),
                content: response.text_output.clone(),
            });
            
            current_messages.push(Message {
                role: "user".to_string(),
                content: "[tool results]".to_string(),
            });
        }
        
        Err(anyhow::anyhow!("Max iterations reached"))
    }
}
