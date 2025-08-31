// src/state.rs

use crate::{
    config::CONFIG,
    git::{GitClient, GitStore},
    llm::{
        client::OpenAIClient,
        responses::{ImageGenerationManager, ResponsesManager, ThreadManager, VectorStoreManager},
    },
    memory::{
        qdrant::multi_store::QdrantMultiStore,
        sqlite::store::SqliteMemoryStore,
    },
    persona::PersonaOverlay,
    project::store::ProjectStore,
    services::{
        chat::ChatConfig, summarization::SummarizationService, ChatService, ContextService,
        DocumentService, FileSearchService, MemoryService,
    },
};
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    // -------- Storage --------
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_multi_store: Arc<QdrantMultiStore>,
    pub project_store: Arc<ProjectStore>,
    pub git_store: GitStore,
    pub git_client: GitClient,

    // -------- LLM Core --------
    pub llm_client: Arc<OpenAIClient>,
    pub responses_manager: Arc<ResponsesManager>,
    pub vector_store_manager: Arc<VectorStoreManager>,
    pub thread_manager: Arc<ThreadManager>,
    pub image_generation_manager: Arc<ImageGenerationManager>,

    // -------- Services --------
    pub chat_service: Arc<ChatService>,
    pub memory_service: Arc<MemoryService>,
    pub context_service: Arc<ContextService>,
    pub document_service: Arc<DocumentService>,
    pub file_search_service: Arc<FileSearchService>,
}

impl AppState {
    /// Assembles the ChatService with all its dependencies.
    pub fn assemble_chat_service(
        llm_client: Arc<OpenAIClient>,
        thread_manager: Arc<ThreadManager>,
        vector_store_manager: Arc<VectorStoreManager>,
        memory_service: Arc<MemoryService>,
        sqlite_store: Arc<SqliteMemoryStore>,
        persona: PersonaOverlay,
        config: Option<ChatConfig>,
    ) -> Arc<ChatService> {
        let chat_config = config.unwrap_or_default();

        let summarization_service = Arc::new(SummarizationService::new_with_stores(
            llm_client.clone(),
            Arc::new(chat_config.clone()),
            sqlite_store.clone(),
            memory_service.clone(),
        ));

        Arc::new(ChatService::new(
            llm_client,
            thread_manager,
            vector_store_manager,
            persona,
            memory_service,
            summarization_service,
            Some(chat_config),
        ))
    }

    /// Wires or re-wires the main chat service with a specific persona and config.
    pub fn wire_chat_service(&mut self, persona: PersonaOverlay, config: Option<ChatConfig>) {
        let chat = Self::assemble_chat_service(
            self.llm_client.clone(),
            self.thread_manager.clone(),
            self.vector_store_manager.clone(),
            self.memory_service.clone(),
            self.sqlite_store.clone(),
            persona,
            config,
        );
        self.chat_service = chat;
    }

    /// Helper to access ImageGenerationManager for tool integration.
    pub fn image_generation_manager(&self) -> &Arc<ImageGenerationManager> {
        &self.image_generation_manager
    }

    /// Helper to access FileSearchService for tool integration.
    pub fn file_search_service(&self) -> &Arc<FileSearchService> {
        &self.file_search_service
    }
}

/// Factory function for creating the application state.
pub async fn create_app_state(
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_url: &str,
    llm_client: Arc<OpenAIClient>,
    project_store: Arc<ProjectStore>,
    git_store: GitStore,
    git_client: GitClient,
) -> anyhow::Result<AppState> {
    info!("🚀 Creating AppState with robust memory features");

    // Initialize multi-collection Qdrant store
    let qdrant_multi_store = Arc::new(QdrantMultiStore::new(qdrant_url, &CONFIG.qdrant_collection).await?);

    // Initialize LLM response managers
    let responses_manager = Arc::new(ResponsesManager::new(llm_client.clone()));
    let vector_store_manager = Arc::new(VectorStoreManager::new(llm_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(
        CONFIG.history_message_cap,
        CONFIG.history_token_limit,
    ));
    let image_generation_manager = Arc::new(ImageGenerationManager::new(llm_client.clone()));

    // Create memory service with multi-store support
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_multi_store.clone(),
        llm_client.clone(),
    ));

    // Create context service with memory service integration
    let context_service = Arc::new(ContextService::new(memory_service.clone()));

    // Initialize document service
    let document_service =
        Arc::new(DocumentService::new(memory_service.clone(), vector_store_manager.clone()));

    // Initialize file search service
    let file_search_service = Arc::new(FileSearchService::new(vector_store_manager.clone(), git_client.clone()));

    // Create default persona for chat service initialization
    let default_persona = PersonaOverlay::mira();

    // Create chat service
    let chat_service = AppState::assemble_chat_service(
        llm_client.clone(),
        thread_manager.clone(),
        vector_store_manager.clone(),
        memory_service.clone(),
        sqlite_store.clone(),
        default_persona,
        None, // Use default ChatConfig
    );

    info!("✅ AppState initialized successfully");

    Ok(AppState {
        sqlite_store,
        qdrant_multi_store,
        project_store,
        git_store,
        git_client,
        llm_client,
        responses_manager,
        vector_store_manager,
        thread_manager,
        image_generation_manager,
        chat_service,
        memory_service,
        context_service,
        document_service,
        file_search_service,
    })
}

/// Back-compat alias for Phase 5 naming (same signature as `create_app_state`).
pub use create_app_state as create_app_state_with_multi_qdrant;
