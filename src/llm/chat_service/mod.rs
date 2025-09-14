// src/llm/chat_service/mod.rs

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, instrument};

pub mod config;
pub mod context;
pub mod response;

pub use config::ChatConfig;
pub use response::ChatResponse;
pub use context::ContextBuilder;
pub use response::ResponseProcessor;

use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::ThreadManager;
use crate::llm::responses::vector_store::VectorStoreManager;
use crate::memory::MemoryService;
use crate::memory::recall::RecallContext;
use crate::persona::PersonaOverlay;

pub struct ChatService {
    client: Arc<OpenAIClient>,
    memory: Arc<MemoryService>,
    context_builder: ContextBuilder,
    response_processor: ResponseProcessor,
    _thread_manager: Arc<ThreadManager>,
    _vector_store_manager: Arc<VectorStoreManager>,
}

impl ChatService {
    pub fn new(
        client: Arc<OpenAIClient>,
        thread_manager: Arc<ThreadManager>,
        vector_store_manager: Arc<VectorStoreManager>,
        persona: PersonaOverlay,
        memory: Arc<MemoryService>,
        config: Option<ChatConfig>,
    ) -> Self {
        let chat_config = config.unwrap_or_default();
        
        info!(
            "Initializing ChatService (model={}, history_cap={}, vector_search={})",
            chat_config.model(),
            chat_config.history_message_cap(),
            chat_config.enable_vector_search()
        );
        
        let context_builder = ContextBuilder::new(
            memory.clone(),
            chat_config.clone(),
        );
        
        let response_processor = ResponseProcessor::new(
            memory.clone(),
            persona.clone(),
            client.clone(),
        );
        
        Self {
            client,
            memory,
            context_builder,
            response_processor,
            _thread_manager: thread_manager,
            _vector_store_manager: vector_store_manager,
        }
    }
    
    #[instrument(skip(self, user_text))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        info!("Starting chat for session: {}", session_id);
        
        // Save user message
        self.response_processor
            .persist_user_message(session_id, user_text, project_id)
            .await?;
        
        // Build context
        let context = self.context_builder
            .build_context_with_fallbacks(session_id, user_text)
            .await?;
        
        // ChatService is deprecated - unified_handler handles streaming now
        // Just create a stub response
        let response_content = String::from("ChatService is deprecated - use unified_handler");
        
        // Process and save response
        let response = self.response_processor
            .process_response(session_id, response_content, &context, project_id)
            .await?;
        
        // Handle rolling summarization
        self.response_processor
            .handle_summarization(session_id)
            .await?;
        
        info!("Chat completed for session: {}", session_id);
        Ok(response)
    }
    
    pub async fn build_recall_context(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        self.context_builder.build_context_with_fallbacks(session_id, user_text).await
    }
    
    fn build_system_prompt(&self, context: &RecallContext) -> String {
        // Simple system prompt - could be enhanced with context
        format!(
            "You are a helpful assistant. Context: {} recent messages, {} semantic matches.",
            context.recent.len(),
            context.semantic.len()
        )
    }
    
    pub fn client(&self) -> &Arc<OpenAIClient> {
        &self.client
    }
    
    pub fn memory(&self) -> &Arc<MemoryService> {
        &self.memory
    }
}
