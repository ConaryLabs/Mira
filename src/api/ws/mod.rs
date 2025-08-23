// src/api/ws/mod.rs
// Updated to reflect proper directory structure after chat.rs refactoring

// Core WebSocket modules (all refactored and properly organized)
pub mod chat;           // Main handler: src/api/ws/chat/mod.rs + extracted modules

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

// Export refactored chat module types for external use (now from chat/ directory)
pub use chat::{
    WebSocketConnection,      // from chat/connection.rs
    MessageRouter,            // from chat/message_router.rs  
    should_use_tools,         // from chat/message_router.rs
    extract_file_context,     // from chat/message_router.rs
    HeartbeatManager,         // from chat/heartbeat.rs
    HeartbeatConfig,          // from chat/heartbeat.rs
    HeartbeatStats,           // from chat/heartbeat.rs
    handle_simple_chat_message, // from chat/mod.rs
    ws_chat_handler,          // from chat/mod.rs
};

pub fn ws_router(app_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/test", get(|| async { 
            eprintln!("HTTP GET to /ws/test");
            "WebSocket routes are loaded!" 
        }))
        .route("/chat", get(chat::ws_chat_handler)) // Uses refactored handler from chat/mod.rs
        .with_state(app_state)
}
