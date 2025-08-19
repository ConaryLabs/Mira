// src/api/ws/mod.rs
pub mod chat;
pub mod persona;
pub mod project;
pub mod message;
pub mod session_state;
pub mod chat_tools;  // ADDED: Phase 3 tool support module

use axum::{Router, routing::get};
use std::sync::Arc;
use crate::state::AppState;

// Export session state types for use elsewhere
pub use session_state::{WsSessionState, WsSessionManager};

// Export tool-related types for use in chat handlers
pub use chat_tools::{
    WsServerMessageWithTools,
    handle_chat_message_with_tools,
    update_ws_handler_for_tools
};

pub fn ws_router(app_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/test", get(|| async { 
            eprintln!("HTTP GET to /ws/test");
            "WebSocket routes are loaded!" 
        }))
        .route("/chat", get(chat::ws_chat_handler))
        .with_state(app_state)
}
