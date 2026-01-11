// crates/mira-server/src/web/api/mod.rs
// REST API handlers

mod code;
mod memory;
mod projects;
mod sessions;
mod tasks;

use axum::{extract::State, response::IntoResponse, Json};
use mira_types::{ApiResponse, WsEvent};

use crate::web::state::AppState;

// Re-export all handlers for router compatibility
pub use code::{get_symbols, semantic_search, trigger_index};
pub use memory::{create_memory, delete_memory, get_memory, list_memories, recall};
pub use projects::{get_persona, get_project, list_projects, set_project, set_session_persona};
pub use sessions::{export_session, get_session, get_session_history, list_sessions};
pub use tasks::{create_goal, create_task, list_goals, list_tasks};

// Re-export chat handlers from chat module
pub use crate::web::chat::{chat, test_chat};

// ═══════════════════════════════════════
// CHAT HISTORY
// ═══════════════════════════════════════

/// Get recent chat history for the UI
pub async fn get_chat_history(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.get_recent_messages(20) {
        Ok(messages) => {
            let history: Vec<serde_json::Value> = messages
                .into_iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "role": m.role,
                        "content": m.content,
                        "timestamp": m.created_at,
                    })
                })
                .collect();
            Json(ApiResponse::ok(history))
        }
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

// ═══════════════════════════════════════
// HEALTH
// ═══════════════════════════════════════

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// ═══════════════════════════════════════
// BROADCAST API (for MCP → WebSocket bridge)
// ═══════════════════════════════════════

/// Receive an event from MCP server and broadcast to WebSocket clients
pub async fn broadcast_event(
    State(state): State<AppState>,
    Json(event): Json<WsEvent>,
) -> impl IntoResponse {
    state.broadcast(event);
    Json(ApiResponse::<()>::ok(()))
}
