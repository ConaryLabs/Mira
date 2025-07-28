use axum::{
    routing::{get, post},
    Router,
};
use tokio::net::TcpListener;
use std::sync::Arc;
use tracing::info;
use tower_http::cors::{CorsLayer, Any};
use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::qdrant::store::QdrantMemoryStore;
use mira_backend::memory;
use mira_backend::handlers::{chat_handler, chat_history_handler, AppState};
use mira_backend::llm::OpenAIClient;
use mira_backend::api::ws::ws_router;
use mira_backend::api::http::{http_router, project_router};
use mira_backend::git::{GitStore, GitClient};
use mira_backend::project::store::ProjectStore;
use sqlx::SqlitePool;
use reqwest::Client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();
    
    // --- Initialize SQLite pool and memory store ---
    let pool = SqlitePool::connect("sqlite://mira.db").await?;
    memory::sqlite::migration::run_migrations(&pool).await?;
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));
    
    // --- Initialize Project store (shares the same pool) ---
    let project_store = Arc::new(ProjectStore::new(pool.clone()));
    
    // --- Initialize Git store and client ---
    let git_store = GitStore::new(pool.clone());
    // Set your desired clone directory (could be config/env if you want)
    let git_client = GitClient::new("./repos", git_store.clone());

    // --- Initialize Qdrant memory store ---
    let qdrant_url = std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string());
    let qdrant_collection = std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "mira-memory".to_string());
    
    // Create collection if it doesn't exist
    let client = Client::new();
    let create_collection_url = format!("{}/collections/{}", qdrant_url, qdrant_collection);
    let _ = client.put(&create_collection_url)
        .json(&serde_json::json!({
            "vectors": {
                "size": 3072,  // GPT-4 embedding size
                "distance": "Cosine"
            }
        }))
        .send()
        .await;
    
    let qdrant_store = Arc::new(QdrantMemoryStore::new(
        client.clone(),
        qdrant_url,
        qdrant_collection,
    ));
    
    // --- Initialize LLM client ---
    let llm_client = Arc::new(OpenAIClient::new());
    
    // --- Create shared app state ---
    let app_state = Arc::new(AppState {
        sqlite_store,
        qdrant_store,
        llm_client,
        project_store: project_store.clone(),
        git_store,
        git_client,
    });
    
    // --- Build CORS layer ---
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    
    // --- Build Axum app with REST, WebSocket, and Project routes ---
    let app = Router::new()
        .route("/", get(|| async { "Mira backend is running!" }))
        .route("/ws-test", get(|| async { "WebSocket routes loaded!" }))
        .route("/chat", post(chat_handler))
        .route("/chat/history", get(chat_history_handler))
        // Classic project & artifact REST endpoints
        .merge(project_router())
        // Unified endpoints: project details, git, etc.
        .merge(http_router())
        // WebSocket routes
        .nest("/ws", ws_router(app_state.clone()))
        .with_state(app_state)
        .layer(cors);
    
    // --- Start the server ---
    let port = 8080;
    let addr = format!("0.0.0.0:{port}");
    info!("🚀 Mira backend listening on http://{addr}");
    info!("📦 SQLite: mira.db");
    info!("🔍 Qdrant: {}", std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string()));
    info!("🌐 WebSocket endpoint: ws://localhost:{}/ws/chat", port);
    info!("📜 Chat history endpoint: http://localhost:{}/chat/history", port);
    info!("📁 Project API: http://localhost:{}/projects", port);
    
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
