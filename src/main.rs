// src/main.rs
use std::sync::Arc;
use std::net::SocketAddr;
use axum::{
    routing::get,
    Router,
};
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;
use sqlx::sqlite::SqlitePoolOptions;
use mira_backend::api::ws::ws_chat_handler;
use mira_backend::config::CONFIG;
use mira_backend::tasks::TaskManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    
    info!("Starting Mira Backend (WebSocket-Only Mode)");
    info!("Model: {}", CONFIG.gpt5_model);
    info!("Tools: {}", if CONFIG.enable_chat_tools { "enabled" } else { "disabled" });
    
    // Create database pool with config-defined connection limit
    let pool = SqlitePoolOptions::new()
        .max_connections(CONFIG.sqlite_max_connections as u32)  // FIXED - use config value (100)
        .connect(&CONFIG.database_url)
        .await?;
    
    info!("SQLite pool created with {} max connections", CONFIG.sqlite_max_connections);
    
    // Initialize all required components for AppState
    let sqlite_store = Arc::new(
        mira_backend::memory::storage::sqlite::store::SqliteMemoryStore::new(pool.clone())
    );
    
    let llm_client = mira_backend::llm::client::OpenAIClient::new()?;
    
    let project_store = Arc::new(
        mira_backend::project::store::ProjectStore::new(pool.clone())
    );
    
    let git_store = mira_backend::git::store::GitStore::new(pool.clone());
    
    let git_client = mira_backend::git::client::GitClient::new(
        CONFIG.git_repos_dir.clone(),
        git_store.clone(),
    );
    
    // Create AppState with all required arguments including SQLite pool
    let app_state = Arc::new(
        mira_backend::state::create_app_state(
            sqlite_store,
            pool.clone(),  // Pass the SQLite pool
            &CONFIG.qdrant_url,
            llm_client,
            project_store,
            git_store,
            git_client,
        ).await?
    );
    
    // Start all background tasks using TaskManager
    let mut task_manager = TaskManager::new(app_state.clone());
    task_manager.start().await;
    info!("Background task manager started");
    
    // Create WebSocket-only router
    let app = Router::new()
        .route("/ws", get(ws_chat_handler))
        .with_state(app_state);
    
    // Start server
    let bind_address = format!("{}:{}", CONFIG.host, CONFIG.port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    
    info!("WebSocket server listening on ws://{}/ws", bind_address);
    info!("Server ready - all HTTP endpoints removed, WebSocket-only mode active");
    
    // Run server
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>()
    ).await?;
    
    // Shutdown tasks on exit
    task_manager.shutdown().await;
    
    Ok(())
}
