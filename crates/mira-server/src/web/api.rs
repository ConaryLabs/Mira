// src/web/api.rs
// REST API handlers

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use mira_types::{
    ApiResponse, CodeSearchRequest, CodeSearchResponse, CodeSearchResult,
    IndexRequest, IndexStats, MemoryFact, ProjectContext, RecallRequest,
    RecallResponse, RememberRequest, WsEvent,
};

use crate::db::parse_memory_fact_row;
use crate::web::state::AppState;

// Re-export handlers from split modules for router compatibility
pub use crate::web::chat::{chat, test_chat};

// ═══════════════════════════════════════
// CHAT HISTORY
// ═══════════════════════════════════════

/// Get recent chat history for the UI
pub async fn get_chat_history(
    State(state): State<AppState>,
) -> impl IntoResponse {
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
// HEALTH & HOME
// ═══════════════════════════════════════

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// ═══════════════════════════════════════
// MEMORY API
// ═══════════════════════════════════════

pub async fn list_memories(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let result: Result<Vec<MemoryFact>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
             FROM memory_facts
             WHERE project_id = ?1 OR ?1 IS NULL
             ORDER BY created_at DESC
             LIMIT 100"
        )?;

        let rows = stmt.query_map([project_id], parse_memory_fact_row)?;

        rows.collect::<Result<Vec<_>, _>>()
    })();

    match result {
        Ok(memories) => Json(ApiResponse::ok(memories)),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn create_memory(
    State(state): State<AppState>,
    Json(req): Json<RememberRequest>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let result: Result<i64, _> = conn.execute(
        "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
        rusqlite::params![
            project_id,
            req.key,
            req.content,
            req.fact_type.unwrap_or_else(|| "general".to_string()),
            req.category,
            req.confidence.unwrap_or(1.0),
        ],
    ).map(|_| conn.last_insert_rowid());

    match result {
        Ok(id) => Json(ApiResponse::ok(serde_json::json!({ "id": id }))),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.conn();

    let result: Result<MemoryFact, _> = conn.query_row(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
         FROM memory_facts WHERE id = ?1",
        [id],
        parse_memory_fact_row,
    );

    match result {
        Ok(memory) => Json(ApiResponse::ok(memory)),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.conn();

    match conn.execute("DELETE FROM memory_facts WHERE id = ?1", [id]) {
        Ok(deleted) => Json(ApiResponse::ok(serde_json::json!({ "deleted": deleted }))),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn recall(
    State(state): State<AppState>,
    Json(req): Json<RecallRequest>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;

    // Try semantic search first if embeddings available
    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(&req.query).await {
            let conn = state.db.conn();
            let limit = req.limit.unwrap_or(10);

            // Convert embedding to bytes for sqlite-vec
            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let result: Result<Vec<MemoryFact>, _> = (|| {
                let mut stmt = conn.prepare(
                    "SELECT f.id, f.project_id, f.key, f.content, f.fact_type, f.category, f.confidence, f.created_at
                     FROM memory_facts f
                     JOIN vec_memory v ON f.id = v.fact_id
                     WHERE (f.project_id = ?1 OR ?1 IS NULL)
                     ORDER BY vec_distance_cosine(v.embedding, ?2)
                     LIMIT ?3"
                )?;

                let rows = stmt.query_map(
                    rusqlite::params![project_id, embedding_bytes, limit],
                    parse_memory_fact_row,
                )?;

                rows.collect::<Result<Vec<_>, _>>()
            })();

            if let Ok(memories) = result {
                if !memories.is_empty() {
                    return Json(ApiResponse::ok(RecallResponse { memories }));
                }
            }
        }
    }

    // Fallback to SQL LIKE search
    let conn = state.db.conn();
    let limit = req.limit.unwrap_or(10);
    let pattern = format!("%{}%", req.query);

    let result: Result<Vec<MemoryFact>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
             FROM memory_facts
             WHERE (project_id = ?1 OR ?1 IS NULL)
               AND content LIKE ?2
             ORDER BY created_at DESC
             LIMIT ?3"
        )?;

        let rows = stmt.query_map(
            rusqlite::params![project_id, pattern, limit],
            parse_memory_fact_row,
        )?;

        rows.collect::<Result<Vec<_>, _>>()
    })();

    match result {
        Ok(memories) => Json(ApiResponse::ok(RecallResponse { memories })),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

// ═══════════════════════════════════════
// CODE API
// ═══════════════════════════════════════

pub async fn get_symbols(
    State(_state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Json<ApiResponse<Vec<mira_types::Symbol>>> {
    let file_path = req.get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if file_path.is_empty() {
        return Json(ApiResponse::err("file_path required"));
    }

    match crate::indexer::extract_symbols(std::path::Path::new(file_path)) {
        Ok(symbols) => {
            let typed_symbols: Vec<mira_types::Symbol> = symbols
                .into_iter()
                .map(|s| mira_types::Symbol {
                    name: s.name,
                    qualified_name: s.qualified_name,
                    symbol_type: s.symbol_type,
                    language: s.language,
                    start_line: s.start_line,
                    end_line: s.end_line,
                    signature: s.signature,
                    visibility: s.visibility,
                    documentation: s.documentation,
                    is_test: s.is_test,
                    is_async: s.is_async,
                })
                .collect();
            Json(ApiResponse::ok(typed_symbols))
        }
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn semantic_search(
    State(state): State<AppState>,
    Json(req): Json<CodeSearchRequest>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;

    // Try semantic search if embeddings available
    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(&req.query).await {
            let conn = state.db.conn();
            let limit = req.limit.unwrap_or(10);

            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let result: Result<Vec<CodeSearchResult>, _> = (|| {
                let mut stmt = conn.prepare(
                    "SELECT file_path, chunk_content, vec_distance_cosine(embedding, ?1) as distance
                     FROM vec_code
                     WHERE project_id = ?2 OR ?2 IS NULL
                     ORDER BY distance
                     LIMIT ?3"
                )?;

                let rows = stmt.query_map(
                    rusqlite::params![embedding_bytes, project_id, limit],
                    |row| {
                        let distance: f64 = row.get(2)?;
                        Ok(CodeSearchResult {
                            file_path: row.get(0)?,
                            line_number: 0, // TODO: Store line numbers in vec_code
                            content: row.get(1)?,
                            symbol_name: None,
                            symbol_type: None,
                            score: (1.0 - distance) as f32,
                        })
                    },
                )?;

                rows.collect::<Result<Vec<_>, _>>()
            })();

            if let Ok(results) = result {
                return Json(ApiResponse::ok(CodeSearchResponse { results }));
            }
        }
    }

    Json(ApiResponse::err("Semantic search requires embeddings API key"))
}

pub async fn trigger_index(
    State(state): State<AppState>,
    Json(req): Json<IndexRequest>,
) -> impl IntoResponse {
    let project = state.get_project().await;

    let path = req.path
        .or_else(|| project.as_ref().map(|p| p.path.clone()))
        .unwrap_or_else(|| ".".to_string());

    let path = std::path::PathBuf::from(&path);

    let project_id = state.project_id().await;

    match crate::indexer::index_project(
        &path,
        state.db.clone(),
        state.embeddings.clone(),
        project_id,
    ).await {
        Ok(stats) => Json(ApiResponse::ok(IndexStats {
            files: stats.files,
            symbols: stats.symbols,
            chunks: stats.chunks,
        })),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

// ═══════════════════════════════════════
// TASKS API
// ═══════════════════════════════════════

pub async fn list_tasks(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let result: Result<Vec<serde_json::Value>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, goal_id, title, description, status, priority, created_at
             FROM tasks
             WHERE project_id = ?1 OR ?1 IS NULL
             ORDER BY created_at DESC
             LIMIT 100"
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

    let title = req.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled");
    let description = req.get("description").and_then(|v| v.as_str());
    let status = req.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
    let priority = req.get("priority").and_then(|v| v.as_str()).unwrap_or("medium");

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

pub async fn list_goals(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let result: Result<Vec<serde_json::Value>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
             FROM goals
             WHERE project_id = ?1 OR ?1 IS NULL
             ORDER BY created_at DESC
             LIMIT 100"
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

    let title = req.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled");
    let description = req.get("description").and_then(|v| v.as_str());
    let status = req.get("status").and_then(|v| v.as_str()).unwrap_or("planning");
    let priority = req.get("priority").and_then(|v| v.as_str()).unwrap_or("medium");

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

// ═══════════════════════════════════════
// SESSION API
// ═══════════════════════════════════════

/// List sessions for a project
pub async fn list_sessions(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    // Get project_id from query params or use active project
    let active_project_id = state.project_id().await;
    let project_id: Option<i64> = params
        .get("project_id")
        .and_then(|s| s.parse().ok())
        .or(active_project_id);

    let project_id = match project_id {
        Some(id) => id,
        None => return Json(ApiResponse::<Vec<serde_json::Value>>::err("project_id required or set active project")),
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
                    let (tool_count, top_tools) = state.db.get_session_stats(&s.id).unwrap_or((0, vec![]));
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
        let session_result: Result<(String, Option<i64>, String, Option<String>, String, String), _> = conn.query_row(
            "SELECT id, project_id, status, summary, started_at, last_activity FROM sessions WHERE id = ?",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        );

        match session_result {
            Ok((id, project_id, status, summary, started_at, last_activity)) => {
                // Get tool count and top tools inline
                let tool_count: usize = conn.query_row(
                    "SELECT COUNT(*) FROM tool_history WHERE session_id = ?",
                    [&session_id],
                    |row| row.get(0),
                ).unwrap_or(0);

                let top_tools: Vec<String> = match conn.prepare(
                    "SELECT tool_name, COUNT(*) as cnt FROM tool_history
                     WHERE session_id = ?
                     GROUP BY tool_name
                     ORDER BY cnt DESC
                     LIMIT 5"
                ) {
                    Ok(mut stmt) => stmt
                        .query_map([&session_id], |row| row.get(0))
                        .map(|rows| rows.filter_map(|r| r.ok()).collect())
                        .unwrap_or_default(),
                    Err(_) => vec![],
                };

                // Get success rate
                let success_rate: f64 = conn.query_row(
                    "SELECT COALESCE(CAST(SUM(success) AS REAL) / NULLIF(COUNT(*), 0), 1.0) FROM tool_history WHERE session_id = ?",
                    [&session_id],
                    |row| row.get(0),
                ).unwrap_or(1.0);

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
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
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
        let session_result: Result<(String, Option<i64>, String, Option<String>, String, String), _> = conn.query_row(
            "SELECT id, project_id, status, summary, started_at, last_activity FROM sessions WHERE id = ?",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        );

        match session_result {
            Ok((id, project_id, status, summary, started_at, last_activity)) => {
                // Get stats inline
                let tool_count: usize = conn.query_row(
                    "SELECT COUNT(*) FROM tool_history WHERE session_id = ?",
                    [&session_id],
                    |row| row.get(0),
                ).unwrap_or(0);

                let top_tools: Vec<String> = match conn.prepare(
                    "SELECT tool_name, COUNT(*) as cnt FROM tool_history
                     WHERE session_id = ?
                     GROUP BY tool_name
                     ORDER BY cnt DESC
                     LIMIT 5"
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
                     ORDER BY created_at ASC"
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
                        |row| Ok(serde_json::json!({
                            "id": row.get::<_, i64>(0)?,
                            "path": row.get::<_, String>(1)?,
                            "name": row.get::<_, Option<String>>(2)?,
                        })),
                    ).ok()
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

// ═══════════════════════════════════════
// PROJECT API
// ═══════════════════════════════════════

/// List all projects
pub async fn list_projects(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let conn = state.db.conn();

    let result: Result<Vec<ProjectContext>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, path, name FROM projects ORDER BY name ASC"
        )?;

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

pub async fn get_project(
    State(state): State<AppState>,
) -> impl IntoResponse {
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
pub async fn get_persona(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let session_persona = state.get_session_persona().await;

    // Get base persona
    let base = state.db.get_base_persona()
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
