// src/state.rs
use std::sync::Arc;

use crate::git::{GitClient, GitStore};
use crate::llm::client::OpenAIClient; // Phase 2: explicit client module path
use crate::llm::responses::{ResponsesManager, ThreadManager, VectorStoreManager};
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::project::store::ProjectStore;
use crate::services::{ChatService, ContextService, DocumentService, MemoryService};

// NOTE: PersonaOverlay must be supplied when constructing ChatService
// (no fallback, no per-request override).
// use crate::persona::PersonaOverlay; // <- import at your construction site, not needed here.

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
}

// If you want a convenience constructor, we can add:
//
// impl AppState {
//     pub fn new(
//         sqlite_store: Arc<SqliteMemoryStore>,
//         qdrant_store: Arc<QdrantMemoryStore>,
//         project_store: Arc<ProjectStore>,
//         git_store: GitStore,
//         git_client: GitClient,
//         llm_client: Arc<OpenAIClient>,
//         responses_manager: Arc<ResponsesManager>,
//         vector_store_manager: Arc<VectorStoreManager>,
//         thread_manager: Arc<ThreadManager>,
//         // IMPORTANT: persona overlay comes from src/persona, passed into ChatService upstream
//         chat_service: Arc<ChatService>,
//         memory_service: Arc<MemoryService>,
//         context_service: Arc<ContextService>,
//         document_service: Arc<DocumentService>,
//     ) -> Self {
//         Self {
//             sqlite_store,
//             qdrant_store,
//             project_store,
//             git_store,
//             git_client,
//             llm_client,
//             responses_manager,
//             vector_store_manager,
//             thread_manager,
//             chat_service,
//             memory_service,
//             context_service,
//             document_service,
//         }
//     }
// }
