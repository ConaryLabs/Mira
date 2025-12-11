// src/tools/workspace.rs
// Workspace tracking tools - file activity and work context

use chrono::Utc;
use sqlx::sqlite::SqlitePool;

use super::types::*;

/// Record file activity
pub async fn record_activity(db: &SqlitePool, req: RecordActivityRequest) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();

    let result = sqlx::query(r#"
        INSERT INTO file_activity (file_path, activity_type, context, created_at)
        VALUES ($1, $2, $3, $4)
    "#)
    .bind(&req.file_path)
    .bind(&req.activity_type)
    .bind(&req.context)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "id": result.last_insert_rowid(),
        "file_path": req.file_path,
        "activity_type": req.activity_type,
    }))
}

/// Get recent file activity
pub async fn get_recent_activity(db: &SqlitePool, req: GetRecentActivityRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(20);

    let query = r#"
        SELECT id, file_path, activity_type, context, session_id,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM file_activity
        WHERE ($1 IS NULL OR file_path LIKE $1)
          AND ($2 IS NULL OR activity_type = $2)
        ORDER BY created_at DESC
        LIMIT $3
    "#;

    let file_pattern = req.file_path.as_ref().map(|f| format!("%{}%", f));
    let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, Option<String>, String)>(query)
        .bind(&file_pattern)
        .bind(&req.activity_type)
        .bind(limit)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, file_path, activity_type, context, session_id, created_at)| {
            serde_json::json!({
                "id": id,
                "file_path": file_path,
                "activity_type": activity_type,
                "context": context,
                "session_id": session_id,
                "created_at": created_at,
            })
        })
        .collect())
}

/// Set work context
pub async fn set_context(db: &SqlitePool, req: SetContextRequest) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let priority = req.priority.unwrap_or(0);
    let expires_at = req.ttl_seconds.map(|ttl| now + ttl);

    sqlx::query(r#"
        INSERT INTO work_context (context_type, context_key, context_value, priority, expires_at, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $6)
        ON CONFLICT(context_type, context_key) DO UPDATE SET
            context_value = excluded.context_value,
            priority = excluded.priority,
            expires_at = excluded.expires_at,
            updated_at = excluded.updated_at
    "#)
    .bind(&req.context_type)
    .bind(&req.key)
    .bind(&req.value)
    .bind(priority)
    .bind(expires_at)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "set",
        "context_type": req.context_type,
        "key": req.key,
        "expires_at": expires_at,
    }))
}

/// Get work context
pub async fn get_context(db: &SqlitePool, req: GetContextRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let now = Utc::now().timestamp();

    let query = r#"
        SELECT id, context_type, context_key, context_value, priority,
               datetime(expires_at, 'unixepoch', 'localtime') as expires_at,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM work_context
        WHERE ($1 IS NULL OR context_type = $1)
          AND (expires_at IS NULL OR expires_at > $2)
        ORDER BY priority DESC, updated_at DESC
    "#;

    let rows = sqlx::query_as::<_, (i64, String, String, String, i64, Option<String>, String)>(query)
        .bind(&req.context_type)
        .bind(now)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, context_type, key, value, priority, expires_at, created_at)| {
            serde_json::json!({
                "id": id,
                "context_type": context_type,
                "key": key,
                "value": value,
                "priority": priority,
                "expires_at": expires_at,
                "created_at": created_at,
            })
        })
        .collect())
}
