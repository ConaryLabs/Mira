// src/main.rs

use std::sync::Arc;
use axum::{
    Router,
    routing::{get, post},
};
use tower_http::cors::{CorsLayer, Any};
use tracing::info;
use mira_backend::{
    api::ws::ws_router,
    api::http::http_router,
    state::AppState,  // Changed: Import from state module instead of handlers
    handlers::{chat_handler, chat_history_handler},  // Removed AppState from here
    llm::OpenAIClient,
    llm::responses::{ResponsesManager, VectorStoreManager, ThreadManager},
    memory::{
        sqlite::store::SqliteMemoryStore,
        qdrant::store::QdrantMemoryStore,
    },
    project::{
        store::ProjectStore,
        project_router,
    },
    git::{GitStore, GitClient},
    services::{ChatService, MemoryService, ContextService, HybridMemoryService, DocumentService},
};
use tokio::net::TcpListener;
use sqlx::SqlitePool;
use reqwest::Client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    // --- Initialize SQLite pool ---
    info!("🚀 Initializing SQLite database...");
    let pool = SqlitePool::connect("sqlite://mira.db").await?;
    mira_backend::memory::sqlite::migration::run_migrations(&pool).await?;

    // --- Initialize Memory Stores ---
    info!("📦 Initializing memory stores...");
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));

    let qdrant_url = std::env::var("QDRANT_URL")
        .unwrap_or_else(|_| "http://localhost:6333".to_string());
    let qdrant_collection = std::env::var("QDRANT_COLLECTION")
        .unwrap_or_else(|_| "mira-memory".to_string());

    // Create Qdrant collection if it doesn't exist
    let client = Client::new();
    let create_collection_url = format!("{}/collections/{}", qdrant_url, qdrant_collection);
    let _ = client.put(&create_collection_url)
        .json(&serde_json::json!({
            "vectors": {
                "size": 3072,
                "distance": "Cosine"
            }
        }))
        .send()
        .await;

    let qdrant_store = Arc::new(QdrantMemoryStore::new(
        client.clone(),
        qdrant_url.clone(),
        qdrant_collection,
    ));

    // --- Initialize LLM Client ---
    info!("🤖 Initializing OpenAI client...");
    let api_key = std::env::var("OPENAI_API_KEY")
        .expect("OPENAI_API_KEY must be set");
    let llm_client = Arc::new(OpenAIClient::new());

    // --- Initialize Project Store ---
    info!("📁 Initializing project store...");
    let project_store = Arc::new(ProjectStore::new(pool.clone()));

    // --- Initialize Git stores ---
    info!("🐙 Initializing Git stores...");
    let git_store = GitStore::new(pool.clone());
    let git_dir = std::env::var("GIT_REPOS_DIR")
        .unwrap_or_else(|_| "./repos".to_string());
    let git_client = GitClient::new(&git_dir, git_store.clone());

    // --- Initialize Services ---
    info!("🛠️ Initializing services...");
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
    ));

    // --- Initialize OpenAI Responses Components ---
    info!("🤖 Initializing OpenAI Responses components...");
    let responses_manager = ResponsesManager::new(llm_client.clone());
    // Note: create_assistant() method may not exist or be needed anymore
    // If initialization is needed, check the ResponsesManager implementation
    let responses_manager = Arc::new(responses_manager);

    let vector_store_manager = Arc::new(VectorStoreManager::new(llm_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(llm_client.clone()));

    // --- Initialize Hybrid Services ---
    info!("🔄 Initializing hybrid memory services...");
    let hybrid_service = Arc::new(HybridMemoryService::new(
        chat_service.clone(),
        memory_service.clone(),
        context_service.clone(),
        responses_manager.clone(),
        thread_manager.clone(), // <-- no vector_store_manager here
    ));

    let document_service = Arc::new(DocumentService::new(
        memory_service.clone(),
        chat_service.clone(),
        vector_store_manager.clone(),
    ));

    // --- Create App State ---
    let app_state = Arc::new(AppState {
        sqlite_store,
        qdrant_store,
        llm_client,
        project_store,
        git_store,
        git_client,
        chat_service,
        memory_service,
        context_service,
        responses_manager,
        vector_store_manager,
        thread_manager,
        hybrid_service,
        document_service,
    });

    // --- Configure CORS ---
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // --- Build the app ---
    let app = Router::new()
        .route("/health", get(|| async {
            serde_json::json!({
                "status": "healthy",
                "version": env!("CARGO_PKG_VERSION"),
                "service": "mira-backend"
            }).to_string()
        }))
        .route("/ws-test", get(|| async { "WebSocket routes loaded!" }))
        .route("/chat", post(chat_handler))
        .route("/chat/history", get(chat_history_handler))
        .merge(project_router())
        .merge(http_router())
        .nest("/ws", ws_router(app_state.clone()))
        .with_state(app_state)
        .layer(cors);

    // --- Start the server ---
    let port = 8080;
    let addr = format!("0.0.0.0:{port}");
    info!("🚀 Mira backend listening on http://{addr}");
    info!("📦 SQLite: mira.db");
    info!("🔍 Qdrant: {}", qdrant_url);
    info!("🤖 OpenAI Responses: Initialized");  // Changed from "Assistant" to "Responses"
    info!("🌐 WebSocket endpoint: ws://localhost:{}/ws/chat", port);
    info!("📜 Chat history endpoint: http://localhost:{}/chat/history", port);
    info!("📁 Project API: http://localhost:{}/projects", port);

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
