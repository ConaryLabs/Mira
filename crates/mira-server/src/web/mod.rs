// src/web/mod.rs
// Web server layer for Mira

pub mod api;
pub mod chat;
pub mod claude;
pub mod claude_api;
pub mod deepseek;
pub mod mcp_http;
pub mod search;
pub mod state;
pub mod ws;

use axum::{
    Router,
    routing::{get, post},
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::web::state::AppState;

/// Create the web server router
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // API routes (REST)
    let api_router = Router::new()
        .route("/health", get(api::health))
        .route("/memories", get(api::list_memories).post(api::create_memory))
        .route("/memories/{id}", get(api::get_memory).delete(api::delete_memory))
        .route("/recall", post(api::recall))
        .route("/symbols", post(api::get_symbols))
        .route("/search/code", post(api::semantic_search))
        .route("/index", post(api::trigger_index))
        .route("/embed-now", post(api::embed_now))
        .route("/tasks", get(api::list_tasks).post(api::create_task))
        .route("/goals", get(api::list_goals).post(api::create_goal))
        .route("/project", get(api::get_project))
        .route("/project/set", post(api::set_project))
        .route("/projects", get(api::list_projects))
        // Session history
        .route("/sessions", get(api::list_sessions))
        .route("/sessions/{id}", get(api::get_session))
        .route("/sessions/{id}/history", get(api::get_session_history))
        .route("/sessions/{id}/export", get(api::export_session))
        // MCP → WebSocket bridge
        .route("/broadcast", post(api::broadcast_event))
        // Chat with DeepSeek Reasoner
        .route("/chat", post(api::chat))
        .route("/chat/stream", post(chat::chat_stream))
        .route("/chat/history", get(api::get_chat_history))
        // Test endpoint for debugging (returns detailed JSON)
        .route("/chat/test", post(api::test_chat))
        // Claude Code management (project-scoped)
        .route("/claude/instances", get(claude_api::list_instances))
        .route("/claude/project", get(claude_api::get_project_instance).delete(claude_api::close_project_instance))
        .route("/claude/task", post(claude_api::send_task))
        .route("/claude/close", post(claude_api::close_by_path))
        // Claude Code legacy endpoints (by instance ID)
        .route("/claude/{id}", get(claude_api::get_claude_status).delete(claude_api::kill_claude))
        // Persona management
        .route("/persona", get(api::get_persona))
        .route("/persona/session", post(api::set_session_persona))
        .with_state(state.clone());

    // MCP over HTTP service
    let mcp_service = mcp_http::create_mcp_service(state.clone());

    Router::new()
        // Health check at root level
        .route("/health", get(api::health))

        // API routes
        .nest("/api", api_router)

        // MCP over HTTP (Streamable HTTP transport)
        .nest_service("/mcp", mcp_service)

        // WebSocket for real-time events
        .route("/ws", get(ws::handler))

        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
