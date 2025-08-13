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
