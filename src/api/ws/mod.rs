// src/api/ws/mod.rs

pub mod chat;
pub mod persona;
pub mod project;
pub mod message;
pub mod session_state;

use axum::{Router, routing::get};
use std::sync::Arc;
use crate::handlers::AppState;

// Export session state types for use elsewhere
pub use session_state::{WsSessionState, WsSessionManager};

pub fn ws_router(app_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/test", get(|| async { 
            eprintln!("HTTP GET to /ws/test");
            "WebSocket routes are loaded!" 
        }))
        .route("/chat", get(chat::ws_chat_handler))
        .with_state(app_state)
}
