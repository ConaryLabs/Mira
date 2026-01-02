// src/web/mod.rs
// Web server layer for Mira Studio

pub mod api;
pub mod chat;
pub mod claude;
pub mod claude_api;
pub mod components;
pub mod deepseek;
pub mod embedded;
pub mod state;
pub mod ws;

use axum::{
    Router,
    routing::{get, post},
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::web::state::AppState;

// Note: Leptos SSR is deferred - we're serving a simple HTML shell
// that loads the WASM frontend from mira-app

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
        .route("/tasks", get(api::list_tasks).post(api::create_task))
        .route("/goals", get(api::list_goals).post(api::create_goal))
        .route("/project", get(api::get_project))
        .route("/project/set", post(api::set_project))
        .route("/projects", get(api::list_projects))
        // MCP → WebSocket bridge
        .route("/broadcast", post(api::broadcast_event))
        // Chat with DeepSeek Reasoner
        .route("/chat", post(api::chat))
        // Test endpoint for debugging (returns detailed JSON)
        .route("/chat/test", post(api::test_chat))
        // Claude Code management
        .route("/claude", post(api::spawn_claude))
        .route("/claude/{id}", get(api::get_claude_status).delete(api::kill_claude))
        .route("/claude/{id}/input", post(api::send_claude_input))
        // Persona management
        .route("/persona", get(api::get_persona))
        .route("/persona/session", post(api::set_session_persona))
        .with_state(state.clone());

    Router::new()
        // Health check at root level
        .route("/health", get(api::health))

        // API routes
        .nest("/api", api_router)

        // WebSocket for Ghost Mode
        .route("/ws", get(ws::handler))

        // Embedded static assets (single binary distribution)
        .nest_service("/assets", embedded::EmbeddedAssets)
        .nest_service("/pkg", embedded::EmbeddedPkg)

        // SPA routes - all serve index.html, client-side routing handles the rest
        .route("/", get(embedded::index_html))
        .route("/ghost", get(embedded::index_html))
        .route("/memories", get(embedded::index_html))
        .route("/code", get(embedded::index_html))
        .route("/tasks", get(embedded::index_html))
        .route("/chat", get(embedded::index_html))

        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
