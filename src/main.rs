// src/main.rs

use std::sync::Arc;
use std::net::SocketAddr;
use std::time::Duration;
use axum::{
    routing::get,
    Router,
};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use sqlx::sqlite::SqlitePoolOptions;

mod api;
mod config;
mod git;
mod llm;
mod memory;
mod project;
mod services;
mod state;
mod utils;
mod persona;

use api::ws::ws_chat_handler;
use config::CONFIG;

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
    
    // Create database pool
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&CONFIG.database_url)
        .await?;
    
    // Initialize all required components for AppState
    let sqlite_store = Arc::new(
        memory::sqlite::store::SqliteMemoryStore::new(pool.clone())
    );
    
    let llm_client = llm::client::OpenAIClient::new()?;
    
    let project_store = Arc::new(
        project::store::ProjectStore::new(pool.clone())
    );
    
    let git_store = git::store::GitStore::new(pool.clone());
    
    let git_client = git::client::GitClient::new(
        CONFIG.git_repos_dir.clone(),
        git_store.clone(),
    );

    // Create AppState with all required arguments
    let app_state = Arc::new(
        state::create_app_state(
            sqlite_store,
            &CONFIG.qdrant_url,
            llm_client,
            project_store,
            git_store,
            git_client,
        ).await?
    );
    
    // 🔴 BUG FIX #1: SPAWN THE MEMORY DECAY SCHEDULER
    // This was built but never actually started - like buying a dishwasher and never plugging it in
    let decay_interval = Duration::from_secs(CONFIG.decay_interval_seconds.unwrap_or(3600)); // Default 1 hour
    let decay_handle = memory::decay_scheduler::spawn_decay_scheduler(
        app_state.clone(), 
        decay_interval
    );
    info!("🫧 Memory decay scheduler spawned - running every {} seconds", decay_interval.as_secs());
    info!("   Old memories will now fade appropriately instead of cluttering recall forever");
    
    // Create WebSocket-only router
    let app = Router::new()
        .route("/ws", get(ws_chat_handler))
        .with_state(app_state);
    
    // Start server
    let bind_address = format!("{}:{}", CONFIG.host, CONFIG.port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    
    info!("WebSocket server listening on ws://{}/ws", bind_address);
    info!("Server ready - all HTTP endpoints removed, WebSocket-only mode active");
    
    // Use axum::serve with make_service_with_connect_info to provide ConnectInfo
    let server_future = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>()
    );
    
    // Run server and decay scheduler concurrently
    // If either fails, we want to know about it
    tokio::select! {
        result = server_future => {
            if let Err(e) = result {
                tracing::error!("Server error: {}", e);
            }
        }
        _ = decay_handle => {
            tracing::warn!("Decay scheduler unexpectedly terminated");
        }
    }
    
    Ok(())
}
