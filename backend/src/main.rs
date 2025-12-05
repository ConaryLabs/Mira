// src/main.rs

use axum::{Router, routing::get};
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

use mira_backend::api::http::{create_auth_router, health_check, readiness_check, liveness_check};
use mira_backend::api::ws::ws_chat_handler;
use mira_backend::config::CONFIG;
use mira_backend::metrics::{init_metrics, metrics_handler};
use mira_backend::state::AppState;
use mira_backend::tasks::TaskManager;
use tower_http::cors::{CorsLayer, Any};

/// Graceful shutdown signal handler for SIGTERM and Ctrl+C
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received, draining connections...");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Initialize Prometheus metrics
    init_metrics();

    info!("Starting Mira Backend");
    info!("Model: Gemini (with variable reasoning effort)");
    info!(
        "Tools: {}",
        if CONFIG.enable_chat_tools {
            "enabled"
        } else {
            "disabled"
        }
    );

    let pool = SqlitePoolOptions::new()
        .max_connections(CONFIG.sqlite_max_connections as u32)
        .connect(&CONFIG.database_url)
        .await?;

    // Set critical PRAGMAs for production
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA synchronous = NORMAL")
        .execute(&pool)
        .await?;
    info!("Database PRAGMAs configured for production");

    // Skip migrations since they were already run with sqlx migrate run
    info!("Using existing database schema");

    // Initialize application state with all components
    let app_state = Arc::new(AppState::new(pool.clone()).await?);

    // Start background task manager
    let mut task_manager = TaskManager::new(app_state.clone());
    task_manager.start().await;

    // Build router with WebSocket, HTTP, and health endpoints
    let app = Router::new()
        .route("/ws", get(ws_chat_handler))
        // Health endpoints for load balancers and Kubernetes
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .route("/live", get(liveness_check))
        // Prometheus metrics endpoint
        .route("/metrics", get(metrics_handler))
        .nest("/api/auth", create_auth_router())
        .layer(CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any))
        .with_state(app_state);

    let bind_address = format!("{}:{}", CONFIG.host, CONFIG.port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;

    info!("WebSocket server listening on ws://{}/ws", bind_address);
    info!("Health endpoints: /health, /ready, /live");
    info!("Metrics endpoint: /metrics");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    info!("Server shutting down gracefully...");
    task_manager.shutdown().await;
    info!("Shutdown complete");

    Ok(())
}
