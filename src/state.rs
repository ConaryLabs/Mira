// src/state.rs
// PHASE 1: Added QdrantMultiStore for GPT-5 Robust Memory multi-collection support
// PHASE 3 UPDATE: Added ImageGenerationManager and FileSearchService to AppState

use std::sync::Arc;

use crate::git::{GitClient, GitStore};
use crate::llm::client::OpenAIClient;
use crate::llm::responses::{ResponsesManager, ThreadManager, VectorStoreManager, ImageGenerationManager};
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::qdrant::multi_store::QdrantMultiStore;  // PHASE 1: New import
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::project::store::ProjectStore;
use crate::services::{ChatService, ContextService, DocumentService, MemoryService, FileSearchService};

use crate::persona::PersonaOverlay;
use crate::services::chat::ChatConfig;
use crate::services::summarization::SummarizationService;

#[derive(Clone)]
pub struct AppState {
    // -------- Storage --------
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_store: Arc<QdrantMemoryStore>,
    pub qdrant_multi_store: Arc<QdrantMultiStore>,  // PHASE 1: New multi-collection store
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
    /// PHASE 1: Enhanced helper to create a ChatService with multi-collection support
    pub fn assemble_chat_service(
        llm_client: Arc<OpenAIClient>,
        thread_manager: Arc<ThreadManager>,
        vector_store_manager: Arc<VectorStoreManager>,
        memory_service: Arc<MemoryService>,
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        persona: PersonaOverlay,
        config: Option<ChatConfig>,
    ) -> Arc<ChatService> {
        let chat_config = config.unwrap_or_default();

        // Summarizer gets stores so summarize_if_needed() does real work.
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
            sqlite_store,
            qdrant_store,
            summarization_service,
            Some(chat_config),
        ))
    }

    /// PHASE 1: New helper to create a MemoryService with multi-collection support
    pub fn assemble_memory_service_with_multi_store(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        qdrant_multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Arc<MemoryService> {
        Arc::new(MemoryService::new_with_multi_store(
            sqlite_store,
            qdrant_store,
            qdrant_multi_store,
            llm_client,
        ))
    }

    /// Helper to create FileSearchService with required dependencies
    pub fn assemble_file_search_service(
        vector_store_manager: Arc<VectorStoreManager>,
        git_client: GitClient,
    ) -> Arc<FileSearchService> {
        Arc::new(FileSearchService::new(vector_store_manager, git_client))
    }

    /// PHASE 1: Enhanced method to wire chat service with multi-collection memory support
    pub fn wire_chat_service(&mut self, persona: PersonaOverlay, config: Option<ChatConfig>) {
        let chat = Self::assemble_chat_service(
            self.llm_client.clone(),
            self.thread_manager.clone(),
            self.vector_store_manager.clone(),
            self.memory_service.clone(),
            self.sqlite_store.clone(),
            self.qdrant_store.clone(),
            persona,
            config,
        );
        self.chat_service = chat;
    }

    /// Helper to access ImageGenerationManager for tool integration
    pub fn image_generation_manager(&self) -> &Arc<ImageGenerationManager> {
        &self.image_generation_manager
    }

    /// Helper to access FileSearchService for tool integration
    pub fn file_search_service(&self) -> &Arc<FileSearchService> {
        &self.file_search_service
    }

    /// PHASE 1: New helper to access QdrantMultiStore
    pub fn qdrant_multi_store(&self) -> &Arc<QdrantMultiStore> {
        &self.qdrant_multi_store
    }
}

// PHASE 1: Factory functions for creating AppState with multi-collection support

/// PHASE 1: Create AppState with properly initialized multi-collection Qdrant support
pub async fn create_app_state_with_multi_qdrant(
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_url: &str,
    llm_client: Arc<OpenAIClient>,
    project_store: Arc<ProjectStore>,
    git_store: GitStore,
    git_client: GitClient,
) -> anyhow::Result<AppState> {
    use crate::config::CONFIG;
    use tracing::{info, warn};

    // Initialize single Qdrant store for backward compatibility
    let qdrant_store = Arc::new(
        QdrantMemoryStore::new(qdrant_url, &CONFIG.qdrant_collection).await?
    );

    // PHASE 1: Initialize multi-collection Qdrant store
    let qdrant_multi_store = if CONFIG.is_robust_memory_enabled() {
        info!("🏗️  Robust memory enabled - initializing multi-collection Qdrant store");
        Arc::new(QdrantMultiStore::new(qdrant_url).await?)
    } else {
        info!("📦 Robust memory disabled - using compatibility multi-store wrapper");
        Arc::new(QdrantMultiStore::from_single_store(qdrant_store.clone()))
    };

    // Initialize LLM response managers
    let responses_manager = Arc::new(ResponsesManager::new());
    let vector_store_manager = Arc::new(VectorStoreManager::new(llm_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new());
    let image_generation_manager = Arc::new(ImageGenerationManager::new());

    // PHASE 1: Create memory service with multi-store support
    let memory_service = AppState::assemble_memory_service_with_multi_store(
        sqlite_store.clone(),
        qdrant_store.clone(),
        qdrant_multi_store.clone(),
        llm_client.clone(),
    );

    // Initialize context service
    let context_service = Arc::new(ContextService::new(
        llm_client.clone(),
        sqlite_store.clone(),
        qdrant_store.clone(),
    ));

    // Initialize document service
    let document_service = Arc::new(DocumentService::new(
        memory_service.clone(),
        vector_store_manager.clone(),
    ));

    // Initialize file search service
    let file_search_service = AppState::assemble_file_search_service(
        vector_store_manager.clone(),
        git_client.clone(),
    );

    // Create default persona for chat service initialization
    let default_persona = PersonaOverlay::new("Assistant", "A helpful AI assistant");
    
    // Create chat service
    let chat_service = AppState::assemble_chat_service(
        llm_client.clone(),
        thread_manager.clone(),
        vector_store_manager.clone(),
        memory_service.clone(),
        sqlite_store.clone(),
        qdrant_store.clone(),
        default_persona,
        None, // Use default ChatConfig
    );

    info!("✅ AppState initialized with {} Qdrant collections", 
        if CONFIG.is_robust_memory_enabled() { "multiple" } else { "single" });

    if CONFIG.is_robust_memory_enabled() {
        let collection_info = qdrant_multi_store.get_collection_info();
        info!("📊 Multi-collection setup:");
        for (head, collection_name) in collection_info {
            info!("   - {}: {}", head.as_str(), collection_name);
        }
    } else {
        warn!("⚠️  Robust memory disabled - using single collection mode");
    }

    Ok(AppState {
        sqlite_store,
        qdrant_store,
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
