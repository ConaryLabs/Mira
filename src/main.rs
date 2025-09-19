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
    
    let sqlite_store = Arc::new(
        mira_backend::memory::storage::sqlite::store::SqliteMemoryStore::new(pool.clone())
    );
    
    let llm_client = mira_backend::llm::client::OpenAIClient::new()?;
    
    let project_store = Arc::new(
        mira_backend::project::store::ProjectStore::new(pool.clone())
    );
    
    let git_store = mira_backend::git::store::GitStore::new(pool.clone());
    
    let app_state = Arc::new(
        mira_backend::state::create_app_state(
            sqlite_store,
            pool.clone(),
            &CONFIG.qdrant_url,
            llm_client,
            project_store,
            git_store,
        ).await?
    );
    
    let mut task_manager = TaskManager::new(app_state.clone());
    task_manager.start().await;
    
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
