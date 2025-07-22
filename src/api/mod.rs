// src/api/mod.rs

pub mod ws;
pub mod http; // leave as stub or implement as needed

use axum::Router;
use std::sync::Arc;
use crate::handlers::AppState;

pub fn api_router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .nest("/ws", ws::ws_router(app_state.clone()))
}
