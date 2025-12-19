//! HTTP server for Studio integration
//!
//! Exposes mira-chat functionality via REST/SSE endpoints:
//! - GET /api/status - Health check
//! - POST /api/chat/stream - SSE streaming chat
//! - POST /api/chat/sync - Synchronous chat (for Claude-to-Mira)
//! - GET /api/messages - Paginated message history

mod chat;
mod handlers;
mod markdown_parser;
mod stream;
mod types;

use anyhow::Result;
use axum::{
    extract::DefaultBodyLimit,
    http::{header, Method},
    routing::{get, post},
    Router,
};
use sqlx::SqlitePool;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use mira_core::semantic::SemanticSearch;
use crate::chat::tools::WebSearchConfig;

// Types available for external use (currently internal only)
pub(crate) use types::{ChatEvent, ChatRequest, MessageBlock, ToolCallResult, UsageInfo};

// ============================================================================
// Server State
// ============================================================================

#[derive(Clone)]
pub struct AppState {
    pub db: Option<SqlitePool>,
    pub semantic: Arc<SemanticSearch>,
    pub api_key: String,
    pub default_reasoning_effort: String,
    pub sync_token: Option<String>, // Bearer token for /api/chat/sync
    pub sync_semaphore: Arc<tokio::sync::Semaphore>, // Limit concurrent sync requests
    pub web_search_config: WebSearchConfig, // Google Custom Search config
}

// ============================================================================
// Routes
// ============================================================================

/// Max request body size for sync endpoint (64KB - allows for project_path + message overhead)
const SYNC_MAX_BODY_BYTES: usize = 64 * 1024;

/// Max concurrent sync requests
const SYNC_MAX_CONCURRENT: usize = 3;

/// Create the router with all endpoints
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    Router::new()
        .route("/api/status", get(handlers::status_handler))
        .route("/api/chat/stream", post(stream::chat_stream_handler))
        .route(
            "/api/chat/sync",
            post(stream::chat_sync_handler).layer(DefaultBodyLimit::max(SYNC_MAX_BODY_BYTES)),
        )
        .route("/api/messages", get(handlers::messages_handler))
        .layer(cors)
        .with_state(state)
}

/// Run the HTTP server
pub async fn run(
    port: u16,
    api_key: String,
    db: Option<SqlitePool>,
    semantic: Arc<SemanticSearch>,
    reasoning_effort: String,
    sync_token: Option<String>,
    web_search_config: WebSearchConfig,
) -> Result<()> {
    let state = AppState {
        db,
        semantic,
        api_key,
        default_reasoning_effort: reasoning_effort,
        sync_token: sync_token.clone(),
        sync_semaphore: Arc::new(tokio::sync::Semaphore::new(SYNC_MAX_CONCURRENT)),
        web_search_config,
    };

    let app = create_router(state);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    println!("Server listening on http://{}", addr);
    if sync_token.is_some() {
        println!("Sync auth:    ENABLED (via MIRA_SYNC_TOKEN)");
    } else {
        println!("Sync auth:    DISABLED (set MIRA_SYNC_TOKEN to enable)");
    }
    println!("Sync limit:   {} concurrent requests", SYNC_MAX_CONCURRENT);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
