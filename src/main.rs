// src/main.rs

use std::sync::Arc;
use std::net::SocketAddr;
use axum::{
    routing::get,
    Router,
};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use sqlx::sqlite::SqlitePoolOptions;

use mira_backend::api::ws::ws_chat_handler;
use mira_backend::config::CONFIG;
use mira_backend::state::AppState;
use mira_backend::tasks::TaskManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    
    info!("Starting Mira Backend");
    info!("Model: {}", CONFIG.gpt5_model);
    info!("Tools: {}", if CONFIG.enable_chat_tools { "enabled" } else { "disabled" });
    
    let pool = SqlitePoolOptions::new()
        .max_connections(CONFIG.sqlite_max_connections as u32)
        .connect(&CONFIG.database_url)
        .await?;
    
    // Set critical PRAGMAs for production
    sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await?;
    sqlx::query("PRAGMA journal_mode = WAL").execute(&pool).await?;
    sqlx::query("PRAGMA synchronous = NORMAL").execute(&pool).await?;
    info!("Database PRAGMAs configured for production");
    
    // Skip migrations since they were already run with sqlx migrate run
    info!("Using existing database schema");
    
    // Initialize application state with all components
    let app_state = Arc::new(AppState::new(pool.clone()).await?);
    
    // Start background task manager
    let mut task_manager = TaskManager::new(app_state.clone());
    task_manager.start().await;
    
    // Build router with WebSocket endpoint
    let app = Router::new()
        .route("/ws", get(ws_chat_handler))
        .with_state(app_state);
    
    let bind_address = format!("{}:{}", CONFIG.host, CONFIG.port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    
    info!("WebSocket server listening on ws://{}/ws", bind_address);
    
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>()
    ).await?;
    
    task_manager.shutdown().await;
    
    Ok(())
}
