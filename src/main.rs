// src/main.rs
// Mira v2.0 - GPT-5 Edition with Responses API
// FIXED: Routing conflict resolved - removed project routes from http_router()

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
    llm::responses::{ResponsesManager, ThreadManager, VectorStoreManager},
    memory::{
        qdrant::store::QdrantMemoryStore,
        sqlite::{migration, store::SqliteMemoryStore},
        traits::MemoryStore,
    },
    persona::PersonaOverlay,
    project::{project_router, store::ProjectStore},
    services::{
        chat::{ChatConfig, ChatService},
        ContextService,
        DocumentService,
        MemoryService,
        summarization::SummarizationService,
    },
    state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize env + logging
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("🚀 Mira v2.0 - GPT-5 Edition");
    info!("📅 August 2025 - Full Autonomy Mode");

    // --- SQLite ---
    info!("📦 Initializing SQLite database...");
    let pool = SqlitePool::connect("sqlite://mira.db").await?;
    migration::run_migrations(&pool).await?;

    // --- Memory stores ---
    info!("🧠 Initializing memory stores...");
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));

    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string());
    let qdrant_collection =
        std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "mira-memory".to_string());

    let qdrant_store = Arc::new(QdrantMemoryStore::new(&qdrant_url, &qdrant_collection).await?);

    // --- OpenAI / GPT-5 ---
    info!("🧠 Initializing OpenAI (GPT-5)...");
    let openai_client = OpenAIClient::new()?;
    info!("   ✅ gpt-5 for conversation");
    info!("   ✅ gpt-image-1 for image generation");
    info!("   ✅ text-embedding-3-large for embeddings");

    // --- Projects / Git ---
    info!("📁 Initializing project store...");
    let project_store = Arc::new(ProjectStore::new(pool.clone()));

    info!("🐙 Initializing Git stores...");
    let git_store = GitStore::new(pool.clone());
    let git_dir = std::env::var("GIT_REPOS_DIR").unwrap_or_else(|_| "./repos".to_string());
    let git_client = GitClient::new(&git_dir, git_store.clone());

    // --- Responses API managers ---
    info!("🔧 Initializing Responses API managers...");
    let responses_manager = Arc::new(ResponsesManager::new(openai_client.clone()));
    let vector_store_manager = Arc::new(VectorStoreManager::new(openai_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(100, 128_000));

    // --- Services ---
    info!("🛠️  Initializing services...");
    let chat_config = ChatConfig {
        max_context_messages: 20,
        enable_memory: true,
        enable_file_context: true,
        enable_summarization: true,
        enable_tools: true,
    };

    let memory_service = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
        openai_client.clone(),
    ));

    let context_service = Arc::new(ContextService::new(
        memory_service.clone(),
        openai_client.clone(),
    ));

    let summarization_service = Arc::new(SummarizationService::new(openai_client.clone()));

    let document_service = Arc::new(DocumentService::new(
        memory_service.clone(),
        openai_client.clone(),
    ));

    let chat_service = Arc::new(ChatService::new(
        openai_client.clone(),
        memory_service.clone(),
        context_service.clone(),
        summarization_service.clone(),
        chat_config,
    ));

    let persona_overlay = Arc::new(PersonaOverlay::new());

    // --- App state ---
    info!("🏗️  Building application state...");
    let app_state = Arc::new(AppState {
        db_pool: pool,
        openai_client,
        project_store,
        git_store: Arc::new(git_store),
        git_client: Arc::new(git_client),
        memory_service,
        context_service,
        chat_service,
        document_service,
        summarization_service,
        persona_overlay,
        responses_manager,
        vector_store_manager,
        thread_manager,
    });

    // --- CORS ---
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // --- Router composition (FIXED: Removed singular/plural conflict) ---
    info!("🛣️  Setting up routes...");
    
    // CRITICAL FIX: Clean separation of concerns
    // - http_router() now ONLY serves /health and /chat endpoints (no project routes)
    // - project_router() handles ALL /projects/* routes in a unified hierarchy
    // - Git routes are nested under /projects/:project_id/git for clean structure
    let api = Router::new()
        .merge(http_router(app_state.clone()))               // REST: /health, /chat, /chat/history ONLY
        .nest("/ws", ws_router(app_state.clone()))           // WS: /ws/chat, /ws/test
        .merge(project_router().with_state(app_state.clone())); // Projects: /projects/* (unified)

    let port = 8080;
    let addr = format!("0.0.0.0:{port}");

    let app = Router::new()
        .route("/", get(|| async { "Mira Backend v2.0 - GPT-5" }))
        .nest("/api", api)
        .layer(cors)
        .with_state(app_state);

    info!("🚀 Server starting on {}", addr);
    info!("🌐 Base:            http://{addr}/");
    info!("🌐 API (REST):      http://{addr}/api/…");
    info!("🔌 WS endpoint:     ws://{addr}/api/ws/chat");
    info!("📁 Project routes:  http://{addr}/api/projects/* (unified hierarchy)");
    info!("🐙 Git routes:      http://{addr}/api/projects/:id/git/* (nested under projects)");

    // --- Start server with ConnectInfo (for WS peer addr, etc.) ---
    let listener = TcpListener::bind(&addr).await?;
    info!("✨ Mira is ready for connections!");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
