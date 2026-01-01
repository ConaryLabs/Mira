// src/web/mod.rs
// Web server layer for Mira Studio

pub mod api;
pub mod components;
pub mod state;
pub mod ws;

use axum::{
    Router,
    routing::{get, post},
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
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
        .with_state(state.clone());

    Router::new()
        // Health check at root level
        .route("/health", get(api::health))

        // API routes
        .nest("/api", api_router)

        // WebSocket for Ghost Mode
        .route("/ws", get(ws::handler))

        // Static assets
        .nest_service("/assets", ServeDir::new("assets"))

        // WASM pkg files
        .nest_service("/pkg", ServeDir::new("pkg"))

        // Leptos SSR pages - fallback to home for now
        .route("/", get(api::home))
        .route("/ghost", get(api::home))
        .route("/memories", get(api::home))
        .route("/code", get(api::home))
        .route("/tasks", get(api::home))

        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
