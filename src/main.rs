// src/main.rs
use axum::{
    routing::{get, post},
    Router,
    extract::Extension,
};
use tokio::net::TcpListener;
use std::sync::Arc;
use tracing::info;
use tower_http::cors::{CorsLayer, Any};
use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::qdrant::store::QdrantMemoryStore;
use mira_backend::memory;
use mira_backend::handlers::{chat_handler, AppState};
use mira_backend::llm::OpenAIClient;
// CORRECT: Use the library crate's API module, NOT `mod api;`
use mira_backend::api::ws::ws_router;
use sqlx::SqlitePool;
use reqwest::Client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();
    
    // --- Initialize SQLite pool and memory store ---
    let pool = SqlitePool::connect("sqlite://mira.db").await?;
    memory::sqlite::migration::run_migrations(&pool).await?;
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool));
    
    // --- Initialize Qdrant memory store ---
    let qdrant_url = std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string());
    let qdrant_collection = std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "mira-memory".to_string());
    
    // Create collection if it doesn't exist
    let client = Client::new();
    let create_collection_url = format!("{}/collections/{}", qdrant_url, qdrant_collection);
    let _ = client.put(&create_collection_url)
        .json(&serde_json::json!({
            "vectors": {
                "size": 3072,  // 🔥 UNLEASHED! Was 1536, now FULL POWER
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
    });
    
    // --- Build CORS layer ---
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    
    // --- Build Axum app with REST and WebSocket routes ---
    let app = Router::new()
        .route("/", get(|| async { "Mira backend is running!" }))
        .route("/ws-test", get(|| async { "WebSocket routes loaded!" }))
        .route("/chat", post(chat_handler))
        // NEW: All /ws/* endpoints via ws_router (from your lib crate)
        .nest("/ws", ws_router(app_state.clone()))
        .layer(Extension(app_state))
        .layer(cors);
    
    // --- Start the server ---
    let port = 8080;
    let addr = format!("0.0.0.0:{port}");
    info!("🚀 Mira backend listening on http://{addr}");
    info!("📦 SQLite: mira.db");
    info!("🔍 Qdrant: {}", std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string()));
    info!("🌐 WebSocket endpoint: ws://localhost:{}/ws/chat", port);
    
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
