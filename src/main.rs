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
    // Anthropic for orchestration
    llm::anthropic_client::AnthropicClient,
    // OpenAI for embeddings and image generation
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
        // Removed Midjourney imports
    },
};
use tokio::net::TcpListener;
use sqlx::SqlitePool;
use reqwest::Client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("🚀 Mira v2.0 - Claude + OpenAI Edition");
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
    info!("   ✅ All beta features enabled");

    // --- Initialize OpenAI (for embeddings and images) ---
    info!("🎨 Initializing OpenAI...");
    let openai_client = Arc::new(OpenAIClient::new());
    info!("   ✅ gpt-image-1 for image generation");
    info!("   ✅ text-embedding-3-large for embeddings");

    // --- Initialize Project Store ---
    info!("📁 Initializing project store...");
    let project_store = Arc::new(ProjectStore::new(pool.clone()));

    // --- Initialize Git stores ---
    info!("🐙 Initializing Git stores...");
    let git_store = GitStore::new(pool.clone());
    let git_dir = std::env::var("GIT_REPOS_DIR")
        .unwrap_or_else(|_| "./repos".to_string());
    let git_client = GitClient::new(&git_dir, git_store.clone());

    // --- Initialize Responses API components ---
    info!("🔧 Initializing Responses API...");
    let responses_manager = Arc::new(ResponsesManager::new(openai_client.clone()));
    let vector_store_manager = Arc::new(VectorStoreManager::new(openai_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(openai_client.clone()));

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
    
    // Chat service with Claude and OpenAI
    info!("🚀 Creating orchestrated chat service...");
    let mut chat_service = ChatService::new(
        anthropic_client.clone(),
        openai_client.clone(),
    );
    chat_service.set_context_service(context_service.clone());
    chat_service.set_memory_service(memory_service.clone());
    let chat_service = Arc::new(chat_service);
    
    info!("   ✅ Claude orchestrates all decisions");
    info!("   ✅ OpenAI handles image generation");
    info!("   ✅ Web search via Claude's native tools");
    
    // Hybrid memory service
    let hybrid_service = Arc::new(HybridMemoryService::new(
        chat_service.clone(),
        memory_service.clone(),
        context_service.clone(),
        responses_manager.clone(),
        thread_manager.clone(),
    ));
    
    // Document service
    let document_service = Arc::new(DocumentService::new(
        memory_service.clone(),
        chat_service.clone(),
        vector_store_manager.clone(),
    ));

    // --- Create AppState ---
    let app_state = Arc::new(AppState {
        sqlite_store,
        qdrant_store,
        llm_client: openai_client,
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

    // --- Build the application router ---
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(|| async { "Mira Backend v2.0 - Claude + OpenAI" }))
        .route("/health", get(|| async { 
            axum::Json(serde_json::json!({
                "status": "healthy",
                "version": env!("CARGO_PKG_VERSION"),
                "service": "mira-backend",
                "engine": "claude+openai"
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
    info!("🧠 Brain: Claude Sonnet 4.0");
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
