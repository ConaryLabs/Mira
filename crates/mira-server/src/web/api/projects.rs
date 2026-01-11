// crates/mira-server/src/web/api/projects.rs
// Project management and persona API handlers

use axum::{extract::State, response::IntoResponse, Json};
use mira_types::{ApiResponse, ProjectContext};

use crate::web::state::AppState;

// ═══════════════════════════════════════
// PROJECT API
// ═══════════════════════════════════════

/// List all projects
pub async fn list_projects(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.conn();

    let result: Result<Vec<ProjectContext>, _> = (|| {
        let mut stmt = conn.prepare("SELECT id, path, name FROM projects ORDER BY name ASC")?;

        let rows = stmt.query_map([], |row| {
            Ok(ProjectContext {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
    })();

    match result {
        Ok(projects) => Json(ApiResponse::ok(projects)),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn get_project(State(state): State<AppState>) -> impl IntoResponse {
    match state.get_project().await {
        Some(project) => Json(ApiResponse::ok(project)),
        None => Json(ApiResponse::err("No active project")),
    }
}

pub async fn set_project(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Json<ApiResponse<ProjectContext>> {
    let path = match req.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return Json(ApiResponse::err("path required")),
    };

    let name = req.get("name").and_then(|v| v.as_str());

    // Get or create project (now returns (id, detected_name))
    let (project_id, project_name) = match state.db.get_or_create_project(path, name) {
        Ok(result) => result,
        Err(e) => return Json(ApiResponse::err(e.to_string())),
    };

    let project = ProjectContext {
        id: project_id,
        path: path.to_string(),
        name: project_name,
    };

    state.set_project(project.clone()).await;

    Json(ApiResponse::ok(project))
}

// ═══════════════════════════════════════
// PERSONA API
// ═══════════════════════════════════════

/// Set session persona overlay (ephemeral, for this session only)
/// Body: { "content": "Be extra terse today" } or { "content": null } to clear
pub async fn set_session_persona(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let content = req.get("content").and_then(|v| {
        if v.is_null() {
            None
        } else {
            v.as_str().map(|s| s.to_string())
        }
    });

    state.set_session_persona(content.clone()).await;

    match content {
        Some(c) => Json(ApiResponse::ok(serde_json::json!({
            "session_persona": c,
            "message": "Session persona set"
        }))),
        None => Json(ApiResponse::ok(serde_json::json!({
            "session_persona": null,
            "message": "Session persona cleared"
        }))),
    }
}

/// Get current persona stack (base, project, session)
pub async fn get_persona(State(state): State<AppState>) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let session_persona = state.get_session_persona().await;

    // Get base persona
    let base = state
        .db
        .get_base_persona()
        .ok()
        .flatten()
        .unwrap_or_else(|| crate::persona::DEFAULT_BASE_PERSONA.to_string());

    // Get project persona if project is active
    let project_persona = if let Some(pid) = project_id {
        state.db.get_project_persona(pid).ok().flatten()
    } else {
        None
    };

    Json(ApiResponse::ok(serde_json::json!({
        "base_persona": base,
        "project_persona": project_persona,
        "session_persona": session_persona,
        "has_project": project_id.is_some(),
    })))
}
