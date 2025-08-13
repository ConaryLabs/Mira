// tests/test_helpers.rs
use std::sync::Arc;
use sqlx::sqlite::SqlitePoolOptions;

use mira_backend::{
    AppState,
    llm::{
        OpenAIClient,
        responses::{
            thread::ThreadManager,
            vector_store::VectorStoreManager,
        },
    },
    memory::{
        sqlite::store::SqliteMemoryStore,
        qdrant::store::QdrantMemoryStore,
    },
    services::{MemoryService, ContextService, DocumentService, ChatService},
    persona::PersonaOverlay,
};

/// Build a minimal, unified AppState for integration tests.
/// Uses in-memory SQLite and a local Qdrant (adjust URL/collection if needed).
pub async fn create_test_app_state() -> Arc<AppState> {
    // 1) SQLite (in-memory)
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(":memory:")
        .await
        .expect("create in-memory sqlite");
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool));

    // 2) Qdrant (async factory returns Self, not Arc/Future)
    //    Point to your test instance or docker compose default.
    let qdrant_store = Arc::new(
        QdrantMemoryStore::new("http://localhost:6334", "mira-test")
            .await
            .expect("create qdrant store"),
    );

    // 3) LLM client (returns Arc<OpenAIClient> inside Result — DO NOT wrap again)
    let llm_client: Arc<OpenAIClient> =
        OpenAIClient::new().expect("create OpenAI client");

    // 4) Infra managers
    let thread_manager        = Arc::new(ThreadManager::new());
    let vector_store_manager  = Arc::new(VectorStoreManager::new(llm_client.clone()));

    // 5) Services (arg order matters per the new signatures)
    let memory_service  = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
        llm_client.clone(),
    ));
    let context_service = Arc::new(ContextService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
    ));
    let chat_service    = Arc::new(ChatService::new(
        llm_client.clone(),
        thread_manager.clone(),
        memory_service.clone(),
        context_service.clone(),
        vector_store_manager.clone(),
        PersonaOverlay::Default, // <- enum variant, not ::default()
    ));
    let document_service = Arc::new(DocumentService::new(
        memory_service.clone(),
        vector_store_manager.clone(),
    ));

    // 6) Final AppState (fields must match src/state.rs)
    Arc::new(AppState {
        sqlite_store,
        qdrant_store,
        llm_client,
        thread_manager,
        vector_store_manager,
        memory_service,
        context_service,
        chat_service,
        document_service,
    })
}
