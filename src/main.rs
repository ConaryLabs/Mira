// src/main.rs
// Mira v2.0 - GPT-5 Edition with Responses API
// CLEANED: Removed all emojis for professional, terminal-friendly logging
// PHASE 3 UPDATE: Added ImageGenerationManager and FileSearchService

use std::sync::Arc;

use axum::{routing::get, Router};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use mira_backend::{
    api::http::http_router,
    api::ws::ws_router,
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

    // --- SQLite ---
    info!("Initializing SQLite database");
    let pool = SqlitePool::connect("sqlite://mira.db").await?;
    migration::run_migrations(&pool).await?;

    // --- Memory stores ---
    info!("Initializing memory stores");
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));

    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string());
    let qdrant_collection =
        std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "mira-memory".to_string());

    let qdrant_store = Arc::new(QdrantMemoryStore::new(&qdrant_url, &qdrant_collection).await?);

    // --- OpenAI / GPT-5 ---
    info!("Initializing OpenAI GPT-5 client");
    let openai_client = OpenAIClient::new()?;
    info!("  - Model: gpt-5 for conversation");
    info!("  - Image: gpt-image-1 for image generation");
    info!("  - Embeddings: text-embedding-3-large");

    // --- Projects / Git ---
    info!("Initializing project store");
    let project_store = Arc::new(ProjectStore::new(pool.clone()));

    info!("Initializing Git client and store");
    let git_store = GitStore::new(pool.clone());
    let git_dir = std::env::var("GIT_REPOS_DIR").unwrap_or_else(|_| "./repos".to_string());
    let git_client = GitClient::new(&git_dir, git_store.clone());

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
        .route("/health", get(|| async { "OK" }))
        .layer(cors)
        .with_state(app_state);

    let listener = TcpListener::bind("0.0.0.0:3001").await?;
    info!("Server listening on http://0.0.0.0:3001");
    info!("WebSocket available at ws://0.0.0.0:3001/ws");
    info!("Health check available at http://0.0.0.0:3001/health");

    axum::serve(listener, app).await?;

    Ok(())
}
