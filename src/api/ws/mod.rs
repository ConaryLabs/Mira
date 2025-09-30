// src/api/ws/mod.rs
use std::sync::Arc;
use axum::{
    extract::{ws::WebSocket, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::StreamExt;
use tracing::info;

use crate::state::AppState;

// WebSocket submodules
pub mod chat;
pub mod message;
pub mod memory;
pub mod project;
pub mod git;
pub mod files;
pub mod filesystem;
pub mod code_intelligence;
pub mod documents;  // NEW: Document processing module

// Re-export key components
pub use chat::ws_chat_handler;

/// Creates the main WebSocket router
pub fn ws_router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(ws_chat_handler))
        .with_state(app_state)
}

/// Manages WebSocket connections and shared state
pub struct WsManager {
    // Placeholder for future shared connection state
}

impl Default for WsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WsManager {
    pub fn new() -> Self {
        Self {}
    }

    /// Processes incoming WebSocket messages
    pub async fn handle_message(&self, msg: String) -> Result<(), anyhow::Error> {
        info!("Handling WebSocket message: {}", msg);
        Ok(())
    }

    /// Creates a subscription channel for broadcasting messages
    pub fn subscribe(&self) -> tokio::sync::mpsc::Receiver<String> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        rx
    }
}

/// Creates a shared WebSocket manager instance
pub fn setup_ws_manager() -> Arc<WsManager> {
    Arc::new(WsManager::new())
}

/// Test WebSocket handler for development and debugging
pub async fn websocket_handler(
    ws: axum::extract::ws::WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_test_socket(socket, state))
}

async fn handle_test_socket(socket: WebSocket, _state: Arc<AppState>) {
    let (_sender, _receiver) = socket.split();
    info!("Test WebSocket connection established");
}
