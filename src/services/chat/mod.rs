// src/services/chat/mod.rs
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, instrument};

// Import extracted modules
pub mod config;
pub mod context;
pub mod response;

// Re-export types for external compatibility
pub use config::ChatConfig;
pub use response::ChatResponse;
pub use context::ContextBuilder;
pub use response::ResponseProcessor;

// Import existing dependencies
use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::ThreadManager;
use crate::llm::responses::vector_store::VectorStoreManager;
use crate::memory::MemoryService;
use crate::services::summarization::SummarizationService;
use crate::memory::recall::RecallContext;
use crate::persona::PersonaOverlay;

/// Main ChatService with refactored modular architecture
pub struct ChatService {
    // Core dependencies
    client: Arc<OpenAIClient>,
    memory: Arc<MemoryService>,
    
    // Extracted components that hold the logic
    context_builder: ContextBuilder,
    response_processor: ResponseProcessor,
    streaming_handler: crate::llm::streaming::StreamingHandler,

    // These fields are kept for compatibility with the AppState struct
    _thread_manager: Arc<ThreadManager>,
    _vector_store_manager: Arc<VectorStoreManager>,
}

impl ChatService {
    /// Create new ChatService, ensuring all components are correctly wired.
    pub fn new(
        client: Arc<OpenAIClient>,
        thread_manager: Arc<ThreadManager>,
        vector_store_manager: Arc<VectorStoreManager>,
        persona: PersonaOverlay,
        memory: Arc<MemoryService>,
        summarizer: Arc<SummarizationService>,
        config: Option<ChatConfig>,
    ) -> Self {
        let chat_config = config.unwrap_or_default();
        
        info!(
            "🚀 Initializing ChatService (model={}, history_cap={}, vector_search={})",
            chat_config.model(),
            chat_config.history_message_cap(),
            chat_config.enable_vector_search()
        );

        let context_builder = ContextBuilder::new(
            memory.clone(),
            chat_config.clone(),
        );

        // FIX: Pass the OpenAIClient to the ResponseProcessor.
        // This allows it to call GPT-5 for structured metadata extraction.
        let response_processor = ResponseProcessor::new(
            memory.clone(),
            summarizer.clone(),
            persona.clone(),
            client.clone(), // This dependency is now correctly injected.
        );

        let streaming_handler = crate::llm::streaming::StreamingHandler::new(
            client.clone(),
        );

        Self {
            client,
            memory,
            context_builder,
            response_processor,
            streaming_handler,
            _thread_manager: thread_manager,
            _vector_store_manager: vector_store_manager,
        }
    }

    /// Main chat method
    #[instrument(skip(self, user_text))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        info!("💬 Starting chat for session: {}", session_id);

        self.response_processor
            .persist_user_message(session_id, user_text, project_id)
            .await?;

        let context = self.context_builder
            .build_context_with_fallbacks(session_id, user_text)
            .await?;

        let response_content = self.streaming_handler
            .generate_response(user_text, &context)
            .await?;

        // The process_response function now has the LLM client it needs
        // to get structured metadata about the response it just generated.
        let response = self.response_processor
            .process_response(session_id, response_content, &context, project_id)
            .await?;

        self.response_processor
            .handle_summarization(session_id)
            .await?;

        info!("✅ Chat completed for session: {}", session_id);
        Ok(response)
    }
    
    /// Public helper for context building
    pub async fn build_recall_context(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        self.context_builder.build_context_with_fallbacks(session_id, user_text).await
    }
    
    // Getters for core dependencies
    pub fn client(&self) -> &Arc<OpenAIClient> {
        &self.client
    }

    pub fn memory(&self) -> &Arc<MemoryService> {
        &self.memory
    }
}
