// src/state.rs
use crate::{
    config::CONFIG,
    git::{GitClient, GitStore},
    llm::{
        chat_service::{ChatConfig, ChatService},
        client::OpenAIClient,
        responses::{ImageGenerationManager, ResponsesManager, ThreadManager, VectorStoreManager},
    },
    memory::{
        features::summarization::SummarizationEngine,
        storage::qdrant::multi_store::QdrantMultiStore,
        storage::sqlite::store::SqliteMemoryStore,
    },
    persona::PersonaOverlay,
    project::store::ProjectStore,
    memory::{
        context::ContextService,
        MemoryService,
    },
};
use crate::tools::file_search::FileSearchService;
use crate::tools::document::DocumentService;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::info;

// Upload session for file transfers
#[derive(Debug, Clone)]
pub struct UploadSession {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub chunks: Vec<Vec<u8>>,
    pub total_size: usize,
    pub received_size: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct AppState {
    // Storage
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub project_store: Arc<ProjectStore>,
    pub git_store: GitStore,
    pub git_client: GitClient,
    
    // LLM Core
    pub llm_client: Arc<OpenAIClient>,
    pub responses_manager: Arc<ResponsesManager>,
    pub image_generation_manager: Arc<ImageGenerationManager>,
    
    // Services
    pub memory_service: Arc<MemoryService>,
    pub file_search_service: Arc<FileSearchService>,
    
    // File upload sessions (single-user, in-memory is fine)
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
}

/// Factory function for creating the application state
pub async fn create_app_state(
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_url: &str,
    llm_client: Arc<OpenAIClient>,
    project_store: Arc<ProjectStore>,
    git_store: GitStore,
    git_client: GitClient,
) -> anyhow::Result<AppState> {
    info!("Creating AppState with robust memory features");
    
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
    let _context_service = Arc::new(ContextService::new(memory_service.clone()));
    
    // Initialize document service
    let _document_service =
        Arc::new(DocumentService::new(memory_service.clone(), vector_store_manager.clone()));
    
    // Initialize file search service
    let file_search_service = Arc::new(FileSearchService::new(vector_store_manager.clone(), git_client.clone()));
    
    // Create default persona for chat service initialization
    let default_persona = PersonaOverlay::Default;
    
    // Create chat service
    let chat_config = ChatConfig::default();
    let summarization_service = Arc::new(SummarizationEngine::new(llm_client.clone()));
    
    let _chat_service = Arc::new(ChatService::new(
        llm_client.clone(),
        thread_manager.clone(),
        vector_store_manager.clone(),
        default_persona,
        memory_service.clone(),
        summarization_service,
        Some(chat_config),
    ));
    
    info!("AppState initialized successfully");
    
    Ok(AppState {
        sqlite_store,
        project_store,
        git_store,
        git_client,
        llm_client,
        responses_manager,
        image_generation_manager,
        memory_service,
        file_search_service,
        upload_sessions: Arc::new(RwLock::new(HashMap::new())),
    })
}
