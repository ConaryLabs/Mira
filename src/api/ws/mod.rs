// src/api/ws/mod.rs

pub mod chat;
pub mod persona;
pub mod project;
pub mod message;

use axum::{Router, routing::get};
use std::sync::Arc;
use crate::handlers::AppState;

pub fn ws_router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/chat", get(chat::ws_chat_handler).with_state(app_state))
}
