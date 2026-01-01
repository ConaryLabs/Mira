// src/web/claude_api.rs
// Claude Code instance management API handlers

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use mira_types::{ApiResponse, ClaudeInputRequest, ClaudeSpawnRequest, ClaudeSpawnResponse, WsEvent};

use crate::web::state::AppState;

/// Spawn a new Claude Code instance
pub async fn spawn_claude(
    State(state): State<AppState>,
    Json(req): Json<ClaudeSpawnRequest>,
) -> impl IntoResponse {
    let working_dir = req
        .working_directory
        .or_else(|| {
            futures::executor::block_on(state.get_project()).map(|p| p.path)
        })
        .unwrap_or_else(|| ".".to_string());

    match state
        .claude_manager
        .spawn(working_dir.clone(), Some(req.initial_prompt))
        .await
    {
        Ok(instance_id) => {
            state.broadcast(WsEvent::ClaudeSpawned {
                instance_id: instance_id.clone(),
                working_dir,
            });
            Json(ApiResponse::ok(ClaudeSpawnResponse { instance_id }))
        }
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

/// Get Claude instance status
pub async fn get_claude_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let running = state.claude_manager.is_running(&id).await;
    Json(ApiResponse::ok(serde_json::json!({
        "instance_id": id,
        "running": running,
    })))
}

/// Kill a Claude instance
pub async fn kill_claude(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.claude_manager.kill(&id).await {
        Ok(_) => {
            state.broadcast(WsEvent::ClaudeStopped {
                instance_id: id.clone(),
            });
            Json(ApiResponse::ok(serde_json::json!({
                "instance_id": id,
                "killed": true,
            })))
        }
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

/// Send input to a Claude instance
pub async fn send_claude_input(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ClaudeInputRequest>,
) -> impl IntoResponse {
    match state.claude_manager.send_input(&id, &req.message).await {
        Ok(_) => Json(ApiResponse::ok(serde_json::json!({
            "sent": true,
        }))),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}
