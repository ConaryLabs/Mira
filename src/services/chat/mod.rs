// src/services/chat/mod.rs
// REFACTORED VERSION - Reduced from ~450-500 lines to ~150 lines
// 
// EXTRACTED MODULES:
// - config.rs: Configuration management using centralized CONFIG
// - context.rs: Context building and recall logic
// - response.rs: Response processing and persistence
// - streaming.rs: Streaming logic and message handling
//
// PRESERVED CRITICAL INTEGRATIONS:
// - ChatService::new() signature for AppState compatibility
// - All public methods used by WebSocket and REST handlers
// - Arc<ChatService> return patterns for thread safety
// - Integration with memory, llm, persona systems

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, instrument};

// Import extracted modules
pub mod config;
pub mod context;
pub mod response;
pub mod streaming;

// Re-export types for external compatibility
pub use config::ChatConfig;
pub use response::ChatResponse;
pub use context::ContextBuilder;
pub use response::ResponseProcessor;
pub use streaming::StreamingHandler;

// Import existing dependencies (preserved)
use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::ThreadManager;
use crate::llm::responses::vector_store::VectorStoreManager;
use crate::services::memory::MemoryService;
use crate::services::summarization::SummarizationService;
use crate::memory::recall::RecallContext;
use crate::memory::traits::MemoryStore;
use crate::persona::PersonaOverlay;

/// Main ChatService with refactored modular architecture
pub struct ChatService {
    // Core dependencies (preserved for compatibility)
    client: Arc<OpenAIClient>,
    thread_manager: Arc<ThreadManager>,
    vector_store_manager: Arc<VectorStoreManager>,
    persona: PersonaOverlay,
    memory: Arc<MemoryService>,
    sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
    qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
    summarizer: Arc<SummarizationService>,
    
    // Extracted components
    config: ChatConfig,
    context_builder: ContextBuilder,
    response_processor: ResponseProcessor,
    streaming_handler: StreamingHandler,
}

impl ChatService {
    /// Create new ChatService - PRESERVED signature for AppState compatibility
    pub fn new(
        client: Arc<OpenAIClient>,
        thread_manager: Arc<ThreadManager>,
        vector_store_manager: Arc<VectorStoreManager>,
        persona: PersonaOverlay,
        memory: Arc<MemoryService>,
        sqlite_store: Arc<dyn MemoryStore + Send + Sync>,
        qdrant_store: Arc<dyn MemoryStore + Send + Sync>,
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
            client.clone(),
            sqlite_store.clone(),
            qdrant_store.clone(),
            chat_config.clone(),
        );

        let response_processor = ResponseProcessor::new(
            memory.clone(),
            summarizer.clone(),
            persona.clone(),
        );

        let streaming_handler = StreamingHandler::new(
            client.clone(),
        );

        Self {
            client,
            thread_manager,
            vector_store_manager,
            persona,
            memory,
            sqlite_store,
            qdrant_store,
            summarizer,
            config: chat_config,
            context_builder,
            response_processor,
            streaming_handler,
        }
    }

    /// Main chat method - PRESERVED interface for WebSocket and REST handlers
    #[instrument(skip(self))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        info!("💬 Starting chat for session: {}", session_id);

        // 1) Persist user message using response processor
        self.response_processor
            .persist_user_message(session_id, user_text, project_id)
            .await?;

        // 2) Build recall context using context builder
        let context = self.context_builder
            .build_context(session_id, user_text)
            .await?;

        // 3) Generate response using streaming handler
        let response_content = self.streaming_handler
            .generate_response(user_text, &context)
            .await?;

        // 4) Process and persist response using response processor
        let response = self.response_processor
            .process_response(session_id, response_content, &context)
            .await?;

        // 5) Trigger summarization if needed
        self.response_processor
            .handle_summarization(session_id)
            .await?;

        info!("✅ Chat completed for session: {}", session_id);
        Ok(response)
    }

    /// Public helper for context building - PRESERVED for external callers
    pub async fn build_recall_context(
        &self,
        session_id: &str,
        user_text: &str,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        self.context_builder.build_context(session_id, user_text).await
    }

    // Getters for extracted components (for advanced usage)
    pub fn config(&self) -> &ChatConfig {
        &self.config
    }

    pub fn persona(&self) -> &PersonaOverlay {
        &self.persona
    }

    // Legacy getters for backward compatibility
    pub fn client(&self) -> &Arc<OpenAIClient> {
        &self.client
    }

    pub fn memory(&self) -> &Arc<MemoryService> {
        &self.memory
    }
}
