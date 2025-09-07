// src/api/ws/mod.rs
// WebSocket API module that manages connection handling, message routing, and session state.
// UPDATED: Added files module for Phase 6

use std::sync::Arc;
use axum::{
    extract::{ws::WebSocket, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::StreamExt;
use tracing::info;

// WebSocket submodules
pub mod chat;
pub mod chat_tools;
pub mod message;
pub mod memory;
pub mod project;
pub mod git;    // PHASE 5: Git operations handler
pub mod files;  // PHASE 6: File transfer handler
pub mod session_state;

// Re-export key components for external access
pub use chat::{
    connection::WebSocketConnection,
    heartbeat::HeartbeatManager,
    message_router::MessageRouter,
    ws_chat_handler,
};
pub use chat_tools::{
    executor::{ToolConfig, ToolExecutor},
    message_handler::ToolMessageHandler,
    prompt_builder::ToolPromptBuilder,
};
pub use message::{MessageMetadata, WsClientMessage, WsServerMessage};
pub use memory::handle_memory_command;
pub use project::handle_project_command;
pub use git::handle_git_command;         // PHASE 5: Export git command handler
pub use files::handle_file_transfer;     // PHASE 6: Export file transfer handler
pub use session_state::{WsSessionState, WsSessionManager};

use crate::state::AppState;

/// Creates the main WebSocket router with all endpoints
pub fn ws_router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(ws_chat_handler))
        .with_state(app_state)
}

/// Manages WebSocket connections and shared state
pub struct WsManager {
    // Placeholder for future shared connection state
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
