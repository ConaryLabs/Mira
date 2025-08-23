// src/api/ws/mod.rs
// Updated to include all refactored WebSocket modules with consistent directory structure

// Core WebSocket modules (all refactored)
pub mod chat;           // Main handler: src/api/ws/chat.rs
pub mod connection;     // Connection management: src/api/ws/connection.rs
pub mod message_router; // Message routing: src/api/ws/message_router.rs 
pub mod heartbeat;      // Heartbeat management: src/api/ws/heartbeat.rs

// Tool support modules (refactored with proper structure)
pub mod chat_tools;     // Tool support: src/api/ws/chat_tools/mod.rs

// Existing modules
pub mod persona;
pub mod project;
pub mod message;
pub mod session_state;

use axum::{Router, routing::get};
use std::sync::Arc;
use crate::state::AppState;

// Export session state types for use elsewhere
pub use session_state::{WsSessionState, WsSessionManager};

// Export tool-related types from the refactored chat_tools module
pub use chat_tools::{
    ToolMessageHandler, WsServerMessageWithTools,
    handle_chat_message_with_tools,
    update_ws_handler_for_tools,
    ToolExecutor, ToolConfig, ToolEvent
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
