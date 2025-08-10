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
    state::AppState,
    handlers::{chat_handler, chat_history_handler},
    // New Anthropic imports
    llm::anthropic_client::AnthropicClient,
    // Keep existing OpenAI for embeddings only
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
    services::{
        ChatService, 
        MemoryService, 
        ContextService, 
        HybridMemoryService, 
        DocumentService,
        // New Midjourney imports
        midjourney_client::MidjourneyClient,
    },
};
use tokio::net::TcpListener;
use sqlx::SqlitePool;
use reqwest::Client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("🚀 Mira v2.0 - Claude + Midjourney Edition");
    info!("📅 August 2025 - Full Autonomy Mode");

    // --- Initialize SQLite pool ---
    info!("📦 Initializing SQLite database...");
    let pool = SqlitePool::connect("sqlite://mira.db").await?;
    mira_backend::memory::sqlite::migration::run_migrations(&pool).await?;

    // --- Initialize Memory Stores ---
    info!("🧠 Initializing memory stores...");
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

    // --- Initialize Anthropic (Primary Brain) ---
    info!("🧠 Initializing Claude (Anthropic)...");
    let anthropic_client = Arc::new(AnthropicClient::new());
    info!("   ✅ Claude Sonnet 4.0 - Primary orchestrator");
    info!("   ✅ Claude Opus 4.1 - Complex reasoning");
    info!("   ✅ All beta features enabled");

    // --- Initialize Midjourney (Vision) ---
    info!("🎨 Initializing Midjourney v6.5...");
    let midjourney_client = Arc::new(MidjourneyClient::new()?);
    info!("   ✅ Image generation with v6.5");
    info!("   ✅ Weird mode up to 3000");
    info!("   ✅ Video, blend, describe, inpaint");

    // --- Keep OpenAI for embeddings only ---
    info!("📊 Initializing OpenAI (embeddings only)...");
    let openai_client = Arc::new(OpenAIClient::new());

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
    info!("🔧 Initializing services...");
    
    // Memory service (unchanged)
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
        openai_client.clone(), // Still using OpenAI for embeddings
    ));
    
    // Context service (fix the constructor call)
    let context_service = Arc::new(ContextService::new(
        sqlite_store.clone(),  // Changed from memory_service
        qdrant_store.clone(),  // Changed from project_store
    ));
    
    // --- NEW: Claude + Midjourney Chat Service ---
    info!("🚀 Creating orchestrated chat service...");
    let mut chat_service = ChatService::new(
        anthropic_client.clone(),
        midjourney_client.clone(),
    );
    chat_service.set_context_service(context_service.clone());
    chat_service.set_memory_service(memory_service.clone());
    let chat_service = Arc::new(chat_service);
    
    info!("   ✅ Claude orchestrates all decisions");
    info!("   ✅ Midjourney handles all visuals");
    info!("   ✅ Zero OpenAI dependency for chat");

    // OpenAI Responses Manager (fix constructor - only needs client)
    let responses_manager = Arc::new(ResponsesManager::new(
        openai_client.clone(),
    ));
    
    // Create vector store and thread managers after responses manager
    let vector_store_manager = Arc::new(VectorStoreManager::new(openai_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(openai_client.clone()));
    
    // Document service (fix the constructor call)
    let document_service = Arc::new(DocumentService::new(
        memory_service.clone(),
        chat_service.clone(),
        vector_store_manager.clone(),
    ));

    // Hybrid memory service (uses Claude for decisions now)
    let hybrid_memory_service = Arc::new(HybridMemoryService::new(
        chat_service.clone(),
        memory_service.clone(),
        context_service.clone(),
        responses_manager.clone(),
        thread_manager.clone(),
    ));

    // --- Create AppState ---
    let app_state = Arc::new(AppState {
        chat_service: chat_service.clone(),
        memory_service: memory_service.clone(),
        context_service: context_service.clone(),
        hybrid_service: hybrid_memory_service.clone(),  // Fixed field name
        project_store: project_store.clone(),
        git_store,
        git_client,
        document_service,
        responses_manager,
        thread_manager,
        sqlite_store,
        qdrant_store,
        llm_client: openai_client.clone(),  // Add missing field
        vector_store_manager,  // Add missing field
    });

    // --- Configure CORS ---
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // --- Build router ---
    let app = Router::new()
        .route("/health", get(|| async {
            axum::Json(serde_json::json!({
                "status": "healthy",
                "version": env!("CARGO_PKG_VERSION"),
                "service": "mira-backend",
                "engine": "claude+midjourney"
            }))
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
    
    info!("════════════════════════════════════════════");
    info!("🚀 Mira backend listening on http://{addr}");
    info!("════════════════════════════════════════════");
    info!("🧠 Brain: Claude Sonnet 4.0 + Opus 4.1");
    info!("🎨 Vision: Midjourney v6.5");
    info!("📊 Embeddings: OpenAI text-embedding-3-large");
    info!("💾 Memory: SQLite + Qdrant");
    info!("🌐 WebSocket: ws://localhost:{}/ws/chat", port);
    info!("════════════════════════════════════════════");
    info!("✨ Mira is fully autonomous and ready!");

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
