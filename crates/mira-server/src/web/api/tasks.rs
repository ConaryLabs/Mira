// crates/mira-server/src/web/api/tasks.rs
// Tasks and Goals API handlers

use axum::{extract::State, response::IntoResponse, Json};
use mira_types::ApiResponse;

use crate::web::state::AppState;

// ═══════════════════════════════════════
// TASKS API
// ═══════════════════════════════════════

pub async fn list_tasks(State(state): State<AppState>) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let result: Result<Vec<serde_json::Value>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, goal_id, title, description, status, priority, created_at
             FROM tasks
             WHERE project_id = ?1 OR ?1 IS NULL
             ORDER BY created_at DESC
             LIMIT 100",
        )?;

        let rows = stmt.query_map([project_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "project_id": row.get::<_, Option<i64>>(1)?,
                "goal_id": row.get::<_, Option<i64>>(2)?,
                "title": row.get::<_, String>(3)?,
                "description": row.get::<_, Option<String>>(4)?,
                "status": row.get::<_, String>(5)?,
                "priority": row.get::<_, String>(6)?,
                "created_at": row.get::<_, String>(7)?,
            }))
        })?;

        rows.collect::<Result<Vec<_>, _>>()
    })();

    match result {
        Ok(tasks) => Json(ApiResponse::ok(tasks)),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn create_task(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let title = req
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled");
    let description = req.get("description").and_then(|v| v.as_str());
    let status = req
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("pending");
    let priority = req
        .get("priority")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");

    let result = conn.execute(
        "INSERT INTO tasks (project_id, title, description, status, priority, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
        rusqlite::params![project_id, title, description, status, priority],
    );

    match result {
        Ok(_) => {
            let id = conn.last_insert_rowid();
            Json(ApiResponse::ok(serde_json::json!({ "id": id })))
        }
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

// ═══════════════════════════════════════
// GOALS API
// ═══════════════════════════════════════

pub async fn list_goals(State(state): State<AppState>) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let result: Result<Vec<serde_json::Value>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
             FROM goals
             WHERE project_id = ?1 OR ?1 IS NULL
             ORDER BY created_at DESC
             LIMIT 100",
        )?;

        let rows = stmt.query_map([project_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "project_id": row.get::<_, Option<i64>>(1)?,
                "title": row.get::<_, String>(2)?,
                "description": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?,
                "priority": row.get::<_, String>(5)?,
                "progress_percent": row.get::<_, i32>(6)?,
                "created_at": row.get::<_, String>(7)?,
            }))
        })?;

        rows.collect::<Result<Vec<_>, _>>()
    })();

    match result {
        Ok(goals) => Json(ApiResponse::ok(goals)),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn create_goal(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let title = req
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled");
    let description = req.get("description").and_then(|v| v.as_str());
    let status = req
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("planning");
    let priority = req
        .get("priority")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");

    let result = conn.execute(
        "INSERT INTO goals (project_id, title, description, status, priority, progress_percent, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, datetime('now'))",
        rusqlite::params![project_id, title, description, status, priority],
    );

    match result {
        Ok(_) => {
            let id = conn.last_insert_rowid();
            Json(ApiResponse::ok(serde_json::json!({ "id": id })))
        }
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}
