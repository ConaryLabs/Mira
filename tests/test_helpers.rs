// tests/test_helpers.rs

use mira_backend::{
    handlers::AppState,
    llm::OpenAIClient,
    llm::assistant::{AssistantManager, VectorStoreManager, ThreadManager},
    memory::{
        sqlite::store::SqliteMemoryStore,
        qdrant::store::QdrantMemoryStore,
    },
    project::store::ProjectStore,
    git::{GitStore, GitClient},
    services::{ChatService, MemoryService, ContextService, HybridMemoryService, DocumentService},
};
use std::sync::Arc;
use sqlx::SqlitePool;
use reqwest::Client;

pub async fn create_test_app_state() -> Arc<AppState> {
    // Use in-memory SQLite for tests
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("Failed to create in-memory SQLite pool");
    
    // Run migrations
    mira_backend::memory::sqlite::migration::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");
    
    // Create stores
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));
    
    // Create a test Qdrant client (can fail gracefully if not running)
    let qdrant_store = Arc::new(QdrantMemoryStore::new(
        Client::new(),
        "http://localhost:6333",
        "test-memory",
    ));
    
    // Create LLM client
    let llm_client = Arc::new(OpenAIClient::new());
    
    // Create project store
    let project_store = Arc::new(ProjectStore::new(pool.clone()));
    
    // Create git stores
    let git_store = GitStore::new(pool.clone());
    let git_client = GitClient::new("./test_repos", git_store.clone());
    
    // Create services
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
        llm_client.clone(),
    ));
    
    let context_service = Arc::new(ContextService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
    ));
    
    // Fix: ChatService now only takes llm_client
    let chat_service = Arc::new(ChatService::new(
        llm_client.clone(),
    ));
    
    // Create assistant components
    let assistant_manager = AssistantManager::new(llm_client.clone());
    // Don't actually create assistant in tests unless needed
    let assistant_manager = Arc::new(assistant_manager);
    
    let vector_store_manager = Arc::new(VectorStoreManager::new(llm_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(llm_client.clone()));
    
    // Create hybrid services
    let hybrid_service = Arc::new(HybridMemoryService::new(
        chat_service.clone(),
        memory_service.clone(),
        context_service.clone(),
        assistant_manager.clone(),
        vector_store_manager.clone(),
        thread_manager.clone(),
    ));
    
    let document_service = Arc::new(DocumentService::new(
        vector_store_manager.clone(),
        memory_service.clone(),
        chat_service.clone(),
    ));
    
    Arc::new(AppState {
        sqlite_store,
        qdrant_store,
        llm_client,
        project_store,
        git_store,
        git_client,
        chat_service,
        memory_service,
        context_service,
        assistant_manager,
        vector_store_manager,
        thread_manager,
        hybrid_service,
        document_service,
    })
}
