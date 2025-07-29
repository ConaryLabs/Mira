// tests/test_helpers.rs

use mira_backend::{
    handlers::AppState,
    memory::{
        sqlite::store::SqliteMemoryStore,
        qdrant::store::QdrantMemoryStore,
    },
    llm::OpenAIClient,
    project::store::ProjectStore,
    git::{GitStore, GitClient},
    services::{ChatService, MemoryService, ContextService},
};
use sqlx::SqlitePool;
use std::sync::Arc;
use reqwest::Client;

/// Creates a complete test AppState with all required services
pub async fn create_test_app_state() -> Arc<AppState> {
    // Load .env file for tests
    dotenv::dotenv().ok();
    
    // Use in-memory SQLite for tests
    let pool = SqlitePool::connect(":memory:").await
        .expect("Failed to create in-memory SQLite pool");
    
    // Run migrations
    mira_backend::memory::sqlite::migration::run_migrations(&pool).await
        .expect("Failed to run migrations");
    
    // Initialize stores
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));
    let project_store = Arc::new(ProjectStore::new(pool.clone()));
    let git_store = GitStore::new(pool.clone());
    let git_client = GitClient::new("./test-repos", git_store.clone());
    
    // For tests, use a test Qdrant collection
    let qdrant_url = std::env::var("QDRANT_URL")
        .unwrap_or_else(|_| "http://localhost:6333".to_string());
    let qdrant_collection = "mira-test-memory".to_string();
    
    let qdrant_store = Arc::new(QdrantMemoryStore::new(
        Client::new(),
        qdrant_url,
        qdrant_collection,
    ));
    
    let llm_client = Arc::new(OpenAIClient::new());
    
    // Initialize services
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
        llm_client.clone(),
    ));
    
    let context_service = Arc::new(ContextService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
    ));
    
    let chat_service = Arc::new(ChatService::new(
        llm_client.clone(),
        memory_service.clone(),
        context_service.clone(),
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
    })
}
