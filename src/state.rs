// src/state.rs
// Phase 3: Remove HybridMemoryService from AppState

use std::sync::Arc;

use crate::git::{GitClient, GitStore};
use crate::llm::client::OpenAIClient;
use crate::llm::responses::{ResponsesManager, ThreadManager, VectorStoreManager};
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::project::store::ProjectStore;
use crate::services::{ChatService, ContextService, DocumentService, MemoryService};

// Added: persona + chat config + summarizer
use crate::persona::PersonaOverlay;
use crate::services::chat::ChatConfig;
use crate::services::summarization::SummarizationService;

// NOTE: PersonaOverlay must be supplied when constructing ChatService
// (no fallback, no per-request override).

#[derive(Clone)]
pub struct AppState {
    // -------- Storage --------
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_store: Arc<QdrantMemoryStore>,
    pub project_store: Arc<ProjectStore>,
    pub git_store: GitStore,
    pub git_client: GitClient,

    // -------- LLM Core --------
    pub llm_client: Arc<OpenAIClient>,
    pub responses_manager: Arc<ResponsesManager>,
    pub vector_store_manager: Arc<VectorStoreManager>,
    pub thread_manager: Arc<ThreadManager>,

    // -------- Services --------
    pub chat_service: Arc<ChatService>,
    pub memory_service: Arc<MemoryService>,
    pub context_service: Arc<ContextService>,
    pub document_service: Arc<DocumentService>,
    
    // REMOVED: pub hybrid_service: Arc<HybridMemoryService>,
}

impl AppState {
    /// Convenience helper: create a ChatService wired with a SummarizationService that
    /// actually loads recent messages and persists compact summaries.
    ///
    /// Use this if you assemble services outside of `AppState` construction and just
    /// need a clean way to get a fully wired ChatService instance.
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

    /// If you already have an AppState struct built and just need to (re)wire the
    /// chat_service in place with a specific persona / config.
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
}
