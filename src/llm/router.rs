// src/llm/router.rs
// LLM router providing unified access to GPT-5 provider

use crate::llm::provider::{LlmProvider, Message, Response, ToolResponse, ToolContext};
use crate::llm::provider::gpt5::Gpt5Provider;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

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
}
