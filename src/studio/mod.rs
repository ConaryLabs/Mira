// src/studio/mod.rs
// Mira Studio API - Chat interface for Claude with memory integration

mod types;
mod context;
mod chat;
mod handlers;
mod claude_code;

use axum::{Router, routing::{get, post}};

// Re-export public types
pub use types::StudioState;

/// Create the studio API router
pub fn router(state: StudioState) -> Router {
    Router::new()
        .route("/status", get(handlers::status_handler))
        .route("/conversations", get(handlers::list_conversations))
        .route("/conversations", post(handlers::create_conversation))
        .route("/conversations/{id}", get(handlers::get_conversation))
        .route("/conversations/{id}/messages", get(handlers::get_messages))
        .route("/chat/stream", post(chat::chat_stream_handler))
        .route("/workspace/events", get(handlers::workspace_events_handler))
        .route("/claude-code/launch", post(handlers::launch_claude_code_handler))
        .with_state(state)
}
