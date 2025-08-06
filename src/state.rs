// src/state.rs
use std::sync::Arc;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::llm::OpenAIClient;
use crate::llm::responses::{ResponsesManager, VectorStoreManager, ThreadManager};  // Updated import
use crate::project::store::ProjectStore;
use crate::git::{GitStore, GitClient};
use crate::services::{ChatService, MemoryService, ContextService, HybridMemoryService, DocumentService};

#[derive(Clone)]
pub struct AppState {
    // Storage fields
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_store: Arc<QdrantMemoryStore>,
    pub llm_client: Arc<OpenAIClient>,
    pub project_store: Arc<ProjectStore>,
    pub git_store: GitStore,
    pub git_client: GitClient,
    
    // Service layer
    pub chat_service: Arc<ChatService>,
    pub memory_service: Arc<MemoryService>,
    pub context_service: Arc<ContextService>,
    
    // Responses API components (renamed from assistant)
    pub responses_manager: Arc<ResponsesManager>,  // Renamed field
    pub vector_store_manager: Arc<VectorStoreManager>,
    pub thread_manager: Arc<ThreadManager>,
    pub hybrid_service: Arc<HybridMemoryService>,
    pub document_service: Arc<DocumentService>,
}
