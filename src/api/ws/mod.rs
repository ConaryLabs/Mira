// src/api/ws/mod.rs
// Updated to include all refactored WebSocket modules from Phase 1-4

// Core WebSocket modules
pub mod chat;           // Refactored main handler (Phase 4)
pub mod connection;     // Extracted connection management (Phase 1)
pub mod message_router; // Extracted message routing (Phase 2) 
pub mod heartbeat;      // Extracted heartbeat management (Phase 3)

// Existing modules
pub mod persona;
pub mod project;
pub mod message;
pub mod session_state;
pub mod chat_tools;     // Tool support module (integrates with message_router)

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

// Export refactored module types for external use
pub use connection::WebSocketConnection;
pub use message_router::{MessageRouter, should_use_tools, extract_file_context};
pub use heartbeat::{HeartbeatManager, HeartbeatConfig, HeartbeatStats};

// Re-export the simple chat handler for message_router compatibility  
pub use chat::handle_simple_chat_message;

pub fn ws_router(app_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/test", get(|| async { 
            eprintln!("HTTP GET to /ws/test");
            "WebSocket routes are loaded!" 
        }))
        .route("/chat", get(chat::ws_chat_handler)) // Uses refactored handler
        .with_state(app_state)
}
