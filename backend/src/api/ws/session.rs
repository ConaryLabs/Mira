// backend/src/api/ws/session.rs
// WebSocket handler for chat session management

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    state::AppState,
};

// ============================================================================
// TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub user_id: Option<String>,
    pub name: Option<String>,
    pub project_path: Option<String>,
    pub last_message_preview: Option<String>,
    pub message_count: i64,
    pub created_at: i64,
    pub last_active: i64,
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    name: Option<String>,
    project_path: Option<String>,
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListSessionsRequest {
    project_path: Option<String>,
    search: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SessionIdRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct UpdateSessionRequest {
    id: String,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ForkSessionRequest {
    source_id: String,
    name: Option<String>,
}

// ============================================================================
// DATABASE OPERATIONS
// ============================================================================

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn create_session(
    pool: &SqlitePool,
    name: Option<String>,
    project_path: Option<String>,
    user_id: Option<String>,
) -> Result<ChatSession, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = now_timestamp();

    sqlx::query(
        r#"
        INSERT INTO chat_sessions (id, user_id, name, project_path, message_count, created_at, last_active)
        VALUES (?, ?, ?, ?, 0, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&user_id)
    .bind(&name)
    .bind(&project_path)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(ChatSession {
        id,
        user_id,
        name,
        project_path,
        last_message_preview: None,
        message_count: 0,
        created_at: now,
        last_active: now,
    })
}

async fn list_sessions(
    pool: &SqlitePool,
    project_path: Option<String>,
    search: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<ChatSession>, sqlx::Error> {
    let limit = limit.unwrap_or(50);

    // Use manual query to avoid type inference issues with query_as!
    let rows = if let Some(path) = project_path {
        sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, i64, i64, i64)>(
            r#"
            SELECT id, user_id, name, project_path, last_message_preview,
                   COALESCE(message_count, 0), created_at, last_active
            FROM chat_sessions
            WHERE project_path = ?
            ORDER BY last_active DESC
            LIMIT ?
            "#,
        )
        .bind(&path)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else if let Some(ref search_term) = search {
        let pattern = format!("%{}%", search_term);
        sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, i64, i64, i64)>(
            r#"
            SELECT id, user_id, name, project_path, last_message_preview,
                   COALESCE(message_count, 0), created_at, last_active
            FROM chat_sessions
            WHERE name LIKE ? OR last_message_preview LIKE ?
            ORDER BY last_active DESC
            LIMIT ?
            "#,
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, i64, i64, i64)>(
            r#"
            SELECT id, user_id, name, project_path, last_message_preview,
                   COALESCE(message_count, 0), created_at, last_active
            FROM chat_sessions
            ORDER BY last_active DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    Ok(rows.into_iter().map(|row| ChatSession {
        id: row.0,
        user_id: row.1,
        name: row.2,
        project_path: row.3,
        last_message_preview: row.4,
        message_count: row.5,
        created_at: row.6,
        last_active: row.7,
    }).collect())
}

async fn get_session(pool: &SqlitePool, id: &str) -> Result<Option<ChatSession>, sqlx::Error> {
    let row = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, i64, i64, i64)>(
        r#"
        SELECT id, user_id, name, project_path, last_message_preview,
               COALESCE(message_count, 0), created_at, last_active
        FROM chat_sessions
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ChatSession {
        id: r.0,
        user_id: r.1,
        name: r.2,
        project_path: r.3,
        last_message_preview: r.4,
        message_count: r.5,
        created_at: r.6,
        last_active: r.7,
    }))
}

async fn update_session(
    pool: &SqlitePool,
    id: &str,
    name: Option<String>,
) -> Result<Option<ChatSession>, sqlx::Error> {
    let now = now_timestamp();

    // Update fields that are provided
    if let Some(ref new_name) = name {
        sqlx::query(
            "UPDATE chat_sessions SET name = ?, last_active = ? WHERE id = ?",
        )
        .bind(new_name)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "UPDATE chat_sessions SET last_active = ? WHERE id = ?",
        )
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    }

    get_session(pool, id).await
}

async fn delete_session(pool: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM chat_sessions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

async fn fork_session(
    pool: &SqlitePool,
    source_id: &str,
    name: Option<String>,
) -> Result<ChatSession, sqlx::Error> {
    // Get source session
    let source = get_session(pool, source_id).await?
        .ok_or_else(|| sqlx::Error::RowNotFound)?;

    // Create new session based on source
    let new_id = Uuid::new_v4().to_string();
    let now = now_timestamp();
    let fork_name = name.unwrap_or_else(|| {
        format!("Fork of {}", source.name.as_deref().unwrap_or(&source.id))
    });

    sqlx::query(
        r#"
        INSERT INTO chat_sessions (id, user_id, name, project_path, message_count, created_at, last_active)
        VALUES (?, ?, ?, ?, 0, ?, ?)
        "#,
    )
    .bind(&new_id)
    .bind(&source.user_id)
    .bind(&fork_name)
    .bind(&source.project_path)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    // Copy messages from source session to new session
    sqlx::query(
        r#"
        INSERT INTO memory_entries (session_id, user_id, parent_id, response_id, role, content,
            model, tokens, cost, reasoning_effort, tags, timestamp, created_at)
        SELECT ?, user_id, parent_id, response_id, role, content,
            model, tokens, cost, reasoning_effort, tags, timestamp, created_at
        FROM memory_entries
        WHERE session_id = ?
        "#,
    )
    .bind(&new_id)
    .bind(source_id)
    .execute(pool)
    .await?;

    // Record the fork relationship
    sqlx::query(
        r#"
        INSERT INTO session_forks (source_session_id, forked_session_id, created_at)
        VALUES (?, ?, ?)
        "#,
    )
    .bind(source_id)
    .bind(&new_id)
    .bind(now)
    .execute(pool)
    .await?;

    // Update message count
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memory_entries WHERE session_id = ?"
    )
    .bind(&new_id)
    .fetch_one(pool)
    .await?;

    sqlx::query("UPDATE chat_sessions SET message_count = ? WHERE id = ?")
        .bind(count)
        .bind(&new_id)
        .execute(pool)
        .await?;

    Ok(ChatSession {
        id: new_id,
        user_id: source.user_id,
        name: Some(fork_name),
        project_path: source.project_path,
        last_message_preview: source.last_message_preview,
        message_count: count,
        created_at: now,
        last_active: now,
    })
}

/// Update session metadata after a message is added
/// Called internally when messages are stored
pub async fn update_session_on_message(
    pool: &SqlitePool,
    session_id: &str,
    message_preview: &str,
) -> Result<(), sqlx::Error> {
    let now = now_timestamp();
    let preview = if message_preview.len() > 100 {
        format!("{}...", &message_preview[..100])
    } else {
        message_preview.to_string()
    };

    sqlx::query(
        r#"
        INSERT INTO chat_sessions (id, message_count, last_message_preview, created_at, last_active)
        VALUES (?, 1, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            message_count = message_count + 1,
            last_message_preview = excluded.last_message_preview,
            last_active = excluded.last_active
        "#,
    )
    .bind(session_id)
    .bind(&preview)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

// ============================================================================
// MAIN ROUTER
// ============================================================================

pub async fn handle_session_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("Processing session command: {}", method);

    let pool = &app_state.sqlite_pool;

    let result = match method {
        "session.create" => {
            let req: CreateSessionRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

            // Auto-provision project from path if provided
            let project_id = if let Some(ref path) = req.project_path {
                match app_state
                    .project_store
                    .get_or_create_by_path(path, req.user_id.clone())
                    .await
                {
                    Ok(project) => {
                        info!("Auto-provisioned project {} for path: {}", project.id, path);
                        Some(project.id)
                    }
                    Err(e) => {
                        info!("Could not auto-provision project for path {}: {}", path, e);
                        None
                    }
                }
            } else {
                None
            };

            let session = create_session(pool, req.name, req.project_path.clone(), req.user_id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to create session: {}", e)))?;

            info!("Created session {} (project_id: {:?})", session.id, project_id);

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "session_created",
                    "session": session,
                    "project_id": project_id
                }),
                request_id: None,
            })
        }

        "session.list" => {
            let req: ListSessionsRequest = serde_json::from_value(params).unwrap_or(ListSessionsRequest {
                project_path: None,
                search: None,
                limit: None,
            });

            let sessions = list_sessions(pool, req.project_path, req.search, req.limit)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to list sessions: {}", e)))?;

            debug!("Listed {} sessions", sessions.len());

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "session_list",
                    "sessions": sessions
                }),
                request_id: None,
            })
        }

        "session.get" => {
            let req: SessionIdRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

            let session = get_session(pool, &req.id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to get session: {}", e)))?
                .ok_or_else(|| ApiError::not_found("Session not found"))?;

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "session",
                    "session": session
                }),
                request_id: None,
            })
        }

        "session.update" => {
            let req: UpdateSessionRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

            let session = update_session(pool, &req.id, req.name)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to update session: {}", e)))?
                .ok_or_else(|| ApiError::not_found("Session not found"))?;

            info!("Updated session {}", session.id);

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "session_updated",
                    "session": session
                }),
                request_id: None,
            })
        }

        "session.delete" => {
            let req: SessionIdRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

            let deleted = delete_session(pool, &req.id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to delete session: {}", e)))?;

            if !deleted {
                return Err(ApiError::not_found("Session not found"));
            }

            info!("Deleted session {}", req.id);

            Ok(WsServerMessage::Status {
                message: format!("Session {} deleted", req.id),
                detail: None,
            })
        }

        "session.fork" => {
            let req: ForkSessionRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

            let session = fork_session(pool, &req.source_id, req.name)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to fork session: {}", e)))?;

            info!("Forked session {} to {}", req.source_id, session.id);

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "session_forked",
                    "session": session,
                    "source_id": req.source_id
                }),
                request_id: None,
            })
        }

        _ => {
            error!("Unknown session method: {}", method);
            return Err(ApiError::bad_request(format!(
                "Unknown session method: {}",
                method
            )));
        }
    };

    match &result {
        Ok(_) => info!("Session command {} completed successfully", method),
        Err(e) => error!("Session command {} failed: {:?}", method, e),
    }

    result
}
