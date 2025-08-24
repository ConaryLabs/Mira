// src/main.rs
// Mira v2.0 - GPT-5 Edition with Responses API
// CLEANED: Removed all emojis for professional, terminal-friendly logging
// PHASE 3 UPDATE: Added ImageGenerationManager and FileSearchService
// FIXED: Removed duplicate /health route that was causing Axum router conflict
// FIXED: Use CONFIG system instead of hardcoded values

use std::sync::Arc;

use axum::{routing::get, Router};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use mira_backend::{
    api::http::http_router,
    api::ws::ws_router,
    config::CONFIG,  // ADDED: Import CONFIG
    git::{GitClient, GitStore},
    llm::client::OpenAIClient,
    llm::responses::{ResponsesManager, ThreadManager, VectorStoreManager, ImageGenerationManager}, // PHASE 3: Added ImageGenerationManager
    memory::{
        qdrant::store::QdrantMemoryStore,
        sqlite::{migration, store::SqliteMemoryStore},
    },
    persona::PersonaOverlay,
    project::{project_router, store::ProjectStore},
    services::{
        chat::{ChatConfig, ChatService},
        ContextService,
        DocumentService,
        MemoryService,
        SummarizationService,
        FileSearchService, // PHASE 3: Added FileSearchService
    },
    state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize env + logging
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("Starting Mira v2.0 - GPT-5 Edition");
    info!("Build Date: August 2025 - Full Autonomy Mode");

    // FIXED: Use CONFIG for database URL
    info!("Initializing SQLite database");
    let pool = SqlitePool::connect(&CONFIG.database_url).await?;
    migration::run_migrations(&pool).await?;

    // --- Memory stores ---
    info!("Initializing memory stores");
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));

    // FIXED: Use CONFIG for Qdrant configuration
    let (qdrant_url, qdrant_collection, _) = CONFIG.qdrant_config();
    let qdrant_store = Arc::new(QdrantMemoryStore::new(&qdrant_url, &qdrant_collection).await?);

    // --- OpenAI / GPT-5 ---
    info!("Initializing OpenAI GPT-5 client");
    let openai_client = OpenAIClient::new()?;
    info!("  - Model: {} for conversation", CONFIG.model);
    info!("  - Image: gpt-image-1 for image generation");
    info!("  - Embeddings: text-embedding-3-large");

    // --- Projects / Git ---
    info!("Initializing project store");
    let project_store = Arc::new(ProjectStore::new(pool.clone()));

    info!("Initializing Git client and store");
    let git_store = GitStore::new(pool.clone());
    // FIXED: Use CONFIG for git directory
    let git_client = GitClient::new(&CONFIG.git_repos_dir, git_store.clone());

    // --- Responses API managers ---
    info!("Initializing Responses API managers");
    let responses_manager = Arc::new(ResponsesManager::new(openai_client.clone()));
    let vector_store_manager = Arc::new(VectorStoreManager::new(openai_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(100, 128_000));

    // PHASE 3 NEW: Initialize ImageGenerationManager
    info!("Initializing Phase 3 services");
    let image_generation_manager = Arc::new(ImageGenerationManager::new(openai_client.clone()));

    // --- Services ---
    info!("Initializing services");
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
        openai_client.clone(),
    ));

    let context_service = Arc::new(ContextService::new(
        openai_client.clone(),
        sqlite_store.clone(),
        qdrant_store.clone(),
    ));

    // Create chat config with default settings
    let chat_config = ChatConfig::default();

    let summarization_service = Arc::new(SummarizationService::new(
        openai_client.clone(),
        Arc::new(chat_config.clone()),
    ));

    let document_service = Arc::new(DocumentService::new(
        memory_service.clone(),
        vector_store_manager.clone(),
    ));

    // PHASE 3 NEW: Initialize FileSearchService
    let file_search_service = Arc::new(FileSearchService::new(
        vector_store_manager.clone(),
        git_client.clone(),
    ));

    let persona_overlay = PersonaOverlay::mira();

    let chat_service = Arc::new(ChatService::new(
        openai_client.clone(),
        thread_manager.clone(),
        vector_store_manager.clone(),
        persona_overlay,
        memory_service.clone(),
        sqlite_store.clone(),
        qdrant_store.clone(),
        summarization_service,
        Some(chat_config),
    ));

    // --- App state ---
    info!("Assembling application state");
    let app_state = Arc::new(AppState {
        sqlite_store,
        qdrant_store,
        project_store,
        git_store,
        git_client,
        llm_client: openai_client,
        responses_manager,
        vector_store_manager,
        thread_manager,
        image_generation_manager, // PHASE 3 NEW
        chat_service,
        memory_service,
        context_service,
        document_service,
        file_search_service, // PHASE 3 NEW
    });

    // --- HTTP server ---
    info!("Configuring HTTP server");
    
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(http_router(app_state.clone()))
        .merge(ws_router(app_state.clone()))
        .merge(project_router())
        // REMOVED: .route("/health", get(|| async { "OK" })) - duplicates http_router health endpoint
        .layer(cors)
        .with_state(app_state);

    // FIXED: Use CONFIG.bind_address() instead of hardcoded port
    let bind_addr = CONFIG.bind_address();
    info!("Server bind address: {}", bind_addr);
    let listener = TcpListener::bind(&bind_addr).await?;
    info!("Server listening on http://{}", bind_addr);
    info!("WebSocket available at ws://{}/ws/chat", bind_addr);
    info!("Health check available at http://{}/health", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
