// src/api/mod.rs

pub mod ws;
pub mod http;
pub mod two_phase; // <-- ADDED THIS LINE
pub mod types;     // <-- ADDED THIS LINE

use axum::Router;
use std::sync::Arc;
use crate::state::AppState;

pub fn api_router(app_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .nest("/ws", ws::ws_router(app_state.clone()))
        // You might want to nest http routes under /api as well
        // .nest("/http", http::http_router(app_state.clone()))
}
