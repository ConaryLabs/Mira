// crates/mira-server/src/web/claude_api.rs
// Claude Code instance management API handlers

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use mira_types::{ApiResponse, WsEvent};

use crate::web::state::AppState;

/// List all running Claude instances
pub async fn list_instances(State(state): State<AppState>) -> impl IntoResponse {
    let instances = state.claude_manager.list_all().await;
    Json(ApiResponse::ok(instances))
}

/// Get Claude instance for current project
pub async fn get_project_instance(State(state): State<AppState>) -> impl IntoResponse {
    match state.get_project().await {
        Some(project) => {
            let has_instance = state.claude_manager.has_instance(&project.path).await;
            let instance_id = state.claude_manager.get_instance_id(&project.path).await;
            Json(ApiResponse::ok(serde_json::json!({
                "project_path": project.path,
                "project_name": project.name,
                "has_instance": has_instance,
                "instance_id": instance_id,
            })))
        }
        None => Json(ApiResponse::err("No project selected")),
    }
}

/// Send task to current project's Claude (spawns if needed)
#[derive(serde::Deserialize)]
pub struct TaskRequest {
    pub task: String,
}

pub async fn send_task(
    State(state): State<AppState>,
    Json(req): Json<TaskRequest>,
) -> impl IntoResponse {
    match state.get_project().await {
        Some(project) => {
            match state.claude_manager.send_task(&project.path, &req.task).await {
                Ok(instance_id) => Json(ApiResponse::ok(serde_json::json!({
                    "instance_id": instance_id,
                    "project_path": project.path,
                    "task_sent": true,
                }))),
                Err(e) => Json(ApiResponse::err(e.to_string())),
            }
        }
        None => Json(ApiResponse::err("No project selected")),
    }
}

/// Close current project's Claude instance
pub async fn close_project_instance(State(state): State<AppState>) -> impl IntoResponse {
    match state.get_project().await {
        Some(project) => {
            match state.claude_manager.close_project(&project.path).await {
                Ok(_) => Json(ApiResponse::ok(serde_json::json!({
                    "project_path": project.path,
                    "closed": true,
                }))),
                Err(e) => Json(ApiResponse::err(e.to_string())),
            }
        }
        None => Json(ApiResponse::err("No project selected")),
    }
}

/// Close Claude instance by project path
#[derive(serde::Deserialize)]
pub struct CloseByPathRequest {
    pub project_path: String,
}

pub async fn close_by_path(
    State(state): State<AppState>,
    Json(req): Json<CloseByPathRequest>,
) -> impl IntoResponse {
    match state.claude_manager.close_project(&req.project_path).await {
        Ok(_) => Json(ApiResponse::ok(serde_json::json!({
            "project_path": req.project_path,
            "closed": true,
        }))),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

// ═══════════════════════════════════════
// LEGACY ENDPOINTS (kept for backwards compatibility)
// ═══════════════════════════════════════

/// Get Claude instance status by ID (legacy)
pub async fn get_claude_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Search through all instances for the ID
    let instances = state.claude_manager.list_all().await;
    let found = instances.iter().find(|i| i.id == id);

    match found {
        Some(info) => Json(ApiResponse::ok(serde_json::json!({
            "instance_id": id,
            "running": info.is_running,
            "project_path": info.project_path,
        }))),
        None => Json(ApiResponse::ok(serde_json::json!({
            "instance_id": id,
            "running": false,
        }))),
    }
}

/// Kill a Claude instance by ID (legacy)
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
