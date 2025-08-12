// src/main.rs
use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};
use reqwest::Client;
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use mira_backend::{
    api::http::http_router,
    api::ws::ws_router,
    git::{GitClient, GitStore},
    llm::client::OpenAIClient, // <- Phase 2 path
    llm::responses::{ResponsesManager, ThreadManager, VectorStoreManager},
    memory::{
        qdrant::store::QdrantMemoryStore,
        sqlite::{migration, store::SqliteMemoryStore},
    },
    persona::PersonaOverlay,
    project::{project_router, store::ProjectStore},
    services::{ChatService, ContextService, DocumentService, MemoryService},
    state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("🚀 Mira v2.0 - GPT‑5 Edition");
    info!("📅 August 2025 - Full Autonomy Mode");

    // --- Initialize SQLite pool ---
    info!("📦 Initializing SQLite database...");
    let pool = SqlitePool::connect("sqlite://mira.db").await?;
    migration::run_migrations(&pool).await?;

    // --- Initialize Memory Stores ---
    info!("🧠 Initializing memory stores...");
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));

    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string());
    let qdrant_collection =
        std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "mira-memory".to_string());

    // Ensure Qdrant collection exists (best-effort)
    let http = Client::new();
    let create_collection_url = format!("{}/collections/{}", qdrant_url, qdrant_collection);
    let _ = http
        .put(&create_collection_url)
        .json(&serde_json::json!({
            "vectors": { "size": 3072, "distance": "Cosine" }
        }))
        .send()
        .await;

    let qdrant_store = Arc::new(QdrantMemoryStore::new(
        http.clone(),
        qdrant_url.clone(),
        qdrant_collection,
    ));

    // --- Initialize OpenAI (GPT‑5 + embeddings + images) ---
    info!("🧠 Initializing OpenAI (GPT‑5)...");
    let openai_client = Arc::new(OpenAIClient::new()?);
    info!("   ✅ gpt-5 for conversation");
    info!("   ✅ gpt-image-1 for image generation");
    info!("   ✅ text-embedding-3-large for embeddings");

    // --- Initialize Project Store ---
    info!("📁 Initializing project store...");
    let project_store = Arc::new(ProjectStore::new(pool.clone()));

    // --- Initialize Git stores ---
    info!("🐙 Initializing Git stores...");
    let git_store = GitStore::new(pool.clone());
    let git_dir = std::env::var("GIT_REPOS_DIR").unwrap_or_else(|_| "./repos".to_string());
    let git_client = GitClient::new(&git_dir, git_store.clone());

    // --- Initialize Responses API components ---
    info!("🔧 Initializing Responses API managers...");
    let responses_manager = Arc::new(ResponsesManager::new(openai_client.clone()));
    let vector_store_manager = Arc::new(VectorStoreManager::new(openai_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new());

    // --- Initialize Services ---
    info!("🔧 Initializing services...");

    // Memory service
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
        openai_client.clone(),
    ));

    // Context service
    let context_service = Arc::new(ContextService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
    ));

    // Persona overlay (required; no per-request override)
    let persona = std::env::var("MIRA_PERSONA")
        .ok()
        .and_then(|s| s.parse::<PersonaOverlay>().ok())
        .unwrap_or(PersonaOverlay::Default);
    info!("🧬 Persona overlay: {}", persona.name());

    // Chat service (GPT‑5 via /responses)
    info!("🚀 Creating GPT‑5 chat service...");
    let chat_service = Arc::new(ChatService::new(
        openai_client.clone(),
        thread_manager.clone(),
        persona,
    ));

    // Document service (no chat_service arg)
    let document_service = Arc::new(DocumentService::new(
        memory_service.clone(),
        vector_store_manager.clone(),
    ));

    // --- Create AppState ---
    let app_state = Arc::new(AppState {
        // Storage
        sqlite_store,
        qdrant_store,
        project_store,
        git_store,
        git_client,

        // LLM core
        llm_client: openai_client,
        responses_manager,
        vector_store_manager,
        thread_manager,

        // Services
        chat_service,
        memory_service,
        context_service,
        document_service,
    });

    // --- Build the application router ---
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let port = 8080;
    let addr = format!("0.0.0.0:{port}");

    let app = Router::new()
        .route("/", get(|| async { "Mira Backend v2.0 - GPT‑5" }))
        .route("/health", get(|| async {
            axum::Json(serde_json::json!({
                "status": "healthy",
                "version": env!("CARGO_PKG_VERSION"),
                "service": "mira-backend",
                "engine": "gpt-5"
            }))
        }))
        .route("/ws-test", get(|| async { "WebSocket routes loaded!" }))
        .route("/chat", post(mira_backend::handlers::chat_handler))
        .route("/chat/history", get(mira_backend::handlers::chat_history_handler))
        .merge(project_router())
        .merge(http_router())
        .nest("/ws", ws_router(app_state.clone()))
        .with_state(app_state.clone())
        .layer(cors);

    // --- Start the server ---
    info!("════════════════════════════════════════════");
    info!("🚀 Mira backend listening on http://{addr}");
    info!("════════════════════════════════════════════");
    info!("🧠 Brain: GPT‑5");
    info!("🎨 Images: OpenAI gpt-image-1");
    info!("📊 Embeddings: OpenAI text-embedding-3-large");
    info!("💾 Memory: SQLite + Qdrant");
    info!("🌐 WebSocket: ws://localhost:{}/ws/chat", port);
    info!("════════════════════════════════════════════");
    info!("✨ Mira is fully autonomous and ready!");

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
