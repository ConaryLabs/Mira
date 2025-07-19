// src/main.rs

use axum::{
    routing::post,
    Router,
    extract::Extension,
};
use tokio::net::TcpListener;
use std::sync::Arc;
use tracing::info;

// All modules are brought in from the library crate (lib.rs)
use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::memory;
use mira_backend::handlers;

use sqlx::SqlitePool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    // --- Initialize SQLite pool and memory store ---
    let pool = SqlitePool::connect("sqlite://mira.db").await?;
    // Run DB migrations to ensure schema matches code
    memory::sqlite::migration::run_migrations(&pool).await?;

    // Initialize the memory store with the pool
    let sqlite_store = SqliteMemoryStore::new(pool);
    let memory_store = Arc::new(sqlite_store);

    // --- Build Axum app with /chat route and injected store ---
    let app = Router::new()
        .route("/chat", post(handlers::chat_handler))
        .layer(Extension(memory_store.clone()));

    // --- Start the server ---
    let port = 8080;
    let addr = format!("0.0.0.0:{port}");
    info!("Listening on http://{addr}");

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
