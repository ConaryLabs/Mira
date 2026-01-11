// crates/mira-server/src/web/api/sessions.rs
// Session history and export API handlers

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use mira_types::ApiResponse;
use std::collections::HashMap;

use crate::web::state::AppState;

/// List sessions for a project
pub async fn list_sessions(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    // Get project_id from query params or use active project
    let active_project_id = state.project_id().await;
    let project_id: Option<i64> = params
        .get("project_id")
        .and_then(|s| s.parse().ok())
        .or(active_project_id);

    let project_id = match project_id {
        Some(id) => id,
        None => {
            return Json(ApiResponse::<Vec<serde_json::Value>>::err(
                "project_id required or set active project",
            ))
        }
    };

    let limit: usize = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);

    match state.db.get_recent_sessions(project_id, limit) {
        Ok(sessions) => {
            let result: Vec<serde_json::Value> = sessions
                .into_iter()
                .map(|s| {
                    // Get stats for each session
                    let (tool_count, top_tools) =
                        state.db.get_session_stats(&s.id).unwrap_or((0, vec![]));
                    serde_json::json!({
                        "id": s.id,
                        "project_id": s.project_id,
                        "status": s.status,
                        "summary": s.summary,
                        "started_at": s.started_at,
                        "last_activity": s.last_activity,
                        "tool_count": tool_count,
                        "top_tools": top_tools,
                    })
                })
                .collect();
            Json(ApiResponse::ok(result))
        }
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

/// Get session details with stats
pub async fn get_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    // Do all DB work in a single lock scope
    let result: Result<serde_json::Value, String> = {
        let conn = state.db.conn();

        // Get session info
        let session_result: Result<
            (
                String,
                Option<i64>,
                String,
                Option<String>,
                String,
                String,
            ),
            _,
        > = conn.query_row(
            "SELECT id, project_id, status, summary, started_at, last_activity FROM sessions WHERE id = ?",
            [&session_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        );

        match session_result {
            Ok((id, project_id, status, summary, started_at, last_activity)) => {
                // Get tool count and top tools inline
                let tool_count: usize = conn
                    .query_row(
                        "SELECT COUNT(*) FROM tool_history WHERE session_id = ?",
                        [&session_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                let top_tools: Vec<String> = match conn.prepare(
                    "SELECT tool_name, COUNT(*) as cnt FROM tool_history
                     WHERE session_id = ?
                     GROUP BY tool_name
                     ORDER BY cnt DESC
                     LIMIT 5",
                ) {
                    Ok(mut stmt) => stmt
                        .query_map([&session_id], |row| row.get(0))
                        .map(|rows| rows.filter_map(|r| r.ok()).collect())
                        .unwrap_or_default(),
                    Err(_) => vec![],
                };

                // Get success rate
                let success_rate: f64 = conn
                    .query_row(
                        "SELECT COALESCE(CAST(SUM(success) AS REAL) / NULLIF(COUNT(*), 0), 1.0) FROM tool_history WHERE session_id = ?",
                        [&session_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(1.0);

                Ok(serde_json::json!({
                    "id": id,
                    "project_id": project_id,
                    "status": status,
                    "summary": summary,
                    "started_at": started_at,
                    "last_activity": last_activity,
                    "stats": {
                        "tool_count": tool_count,
                        "top_tools": top_tools,
                        "success_rate": success_rate,
                    }
                }))
            }
            Err(_) => Err(format!("Session not found: {}", session_id)),
        }
    };

    match result {
        Ok(data) => Json(ApiResponse::ok(data)),
        Err(e) => Json(ApiResponse::err(e)),
    }
}

/// Get tool history for a session
pub async fn get_session_history(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let limit: usize = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    match state.db.get_session_history(&session_id, limit) {
        Ok(history) => {
            let result: Vec<serde_json::Value> = history
                .into_iter()
                .map(|h| {
                    serde_json::json!({
                        "id": h.id,
                        "session_id": h.session_id,
                        "tool_name": h.tool_name,
                        "arguments": h.arguments,
                        "result_summary": h.result_summary,
                        "success": h.success,
                        "created_at": h.created_at,
                    })
                })
                .collect();
            Json(ApiResponse::ok(result))
        }
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

/// Export session as JSON (full details)
pub async fn export_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    // Do all DB work in a single lock scope
    let result: Result<serde_json::Value, String> = {
        let conn = state.db.conn();

        // Get session info
        let session_result: Result<
            (
                String,
                Option<i64>,
                String,
                Option<String>,
                String,
                String,
            ),
            _,
        > = conn.query_row(
            "SELECT id, project_id, status, summary, started_at, last_activity FROM sessions WHERE id = ?",
            [&session_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        );

        match session_result {
            Ok((id, project_id, status, summary, started_at, last_activity)) => {
                // Get stats inline
                let tool_count: usize = conn
                    .query_row(
                        "SELECT COUNT(*) FROM tool_history WHERE session_id = ?",
                        [&session_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                let top_tools: Vec<String> = match conn.prepare(
                    "SELECT tool_name, COUNT(*) as cnt FROM tool_history
                     WHERE session_id = ?
                     GROUP BY tool_name
                     ORDER BY cnt DESC
                     LIMIT 5",
                ) {
                    Ok(mut stmt) => stmt
                        .query_map([&session_id], |row| row.get(0))
                        .map(|rows| rows.filter_map(|r| r.ok()).collect())
                        .unwrap_or_default(),
                    Err(_) => vec![],
                };

                // Get full history inline
                let history_json: Vec<serde_json::Value> = match conn.prepare(
                    "SELECT id, tool_name, arguments, result_summary, success, created_at
                     FROM tool_history
                     WHERE session_id = ?
                     ORDER BY created_at ASC",
                ) {
                    Ok(mut hist_stmt) => hist_stmt
                        .query_map([&session_id], |row| {
                            Ok(serde_json::json!({
                                "id": row.get::<_, i64>(0)?,
                                "tool_name": row.get::<_, String>(1)?,
                                "arguments": row.get::<_, Option<String>>(2)?,
                                "result_summary": row.get::<_, Option<String>>(3)?,
                                "success": row.get::<_, i32>(4)? != 0,
                                "created_at": row.get::<_, String>(5)?,
                            }))
                        })
                        .map(|rows| rows.filter_map(|r| r.ok()).collect())
                        .unwrap_or_default(),
                    Err(_) => vec![],
                };

                // Get project info if available
                let project_info: Option<serde_json::Value> = project_id.and_then(|pid| {
                    conn.query_row(
                        "SELECT id, path, name FROM projects WHERE id = ?",
                        [pid],
                        |row| {
                            Ok(serde_json::json!({
                                "id": row.get::<_, i64>(0)?,
                                "path": row.get::<_, String>(1)?,
                                "name": row.get::<_, Option<String>>(2)?,
                            }))
                        },
                    )
                    .ok()
                });

                Ok(serde_json::json!({
                    "session": {
                        "id": id,
                        "project_id": project_id,
                        "status": status,
                        "summary": summary,
                        "started_at": started_at,
                        "last_activity": last_activity,
                    },
                    "project": project_info,
                    "stats": {
                        "tool_count": tool_count,
                        "top_tools": top_tools,
                    },
                    "history": history_json,
                    "exported_at": chrono::Utc::now().to_rfc3339(),
                }))
            }
            Err(_) => Err(format!("Session not found: {}", session_id)),
        }
    };

    match result {
        Ok(data) => Json(ApiResponse::ok(data)),
        Err(e) => Json(ApiResponse::err(e)),
    }
}
