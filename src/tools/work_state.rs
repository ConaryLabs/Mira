// src/tools/work_state.rs
// Work state management for seamless session resume
// Tracks transient state like active todos, plans, and working documents

use chrono::Utc;
use sqlx::sqlite::SqlitePool;

use super::types::{SyncWorkStateRequest, GetWorkStateRequest};

// === Structs ===

#[derive(Debug, Clone)]
pub struct ActivePlan {
    pub status: String,
    pub content: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct WorkingDoc {
    pub filename: String,
    pub path: String,
    pub preview: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkStateTodo {
    pub content: String,
    pub status: String,
    #[serde(rename = "activeForm")]
    pub active_form: String,
}

// === Functions ===

/// Sync work state for seamless session resume
/// Stores transient state like active todos, current file focus, etc.
pub async fn sync_work_state(
    db: &SqlitePool,
    req: SyncWorkStateRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let ttl_hours = req.ttl_hours.unwrap_or(24);
    let expires_at = now + (ttl_hours * 3600);

    let context_value = serde_json::to_string(&req.context_value)?;

    sqlx::query(r#"
        INSERT INTO work_context (context_type, context_key, context_value, priority, expires_at, created_at, updated_at, project_id)
        VALUES ($1, $2, $3, 0, $4, $5, $5, $6)
        ON CONFLICT(context_type, context_key) DO UPDATE SET
            context_value = excluded.context_value,
            expires_at = excluded.expires_at,
            updated_at = excluded.updated_at,
            project_id = COALESCE(excluded.project_id, work_context.project_id)
    "#)
    .bind(&req.context_type)
    .bind(&req.context_key)
    .bind(&context_value)
    .bind(expires_at)
    .bind(now)
    .bind(project_id)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "synced",
        "context_type": req.context_type,
        "context_key": req.context_key,
        "expires_at": expires_at,
    }))
}

/// Get work state for session resume
pub async fn get_work_state(
    db: &SqlitePool,
    req: GetWorkStateRequest,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let now = Utc::now().timestamp();
    let include_expired = req.include_expired.unwrap_or(false);

    let mut query = String::from(r#"
        SELECT context_type, context_key, context_value, expires_at,
               datetime(updated_at, 'unixepoch', 'localtime') as updated
        FROM work_context
        WHERE (project_id IS NULL OR project_id = $1)
    "#);

    if let Some(ref ct) = req.context_type {
        query.push_str(&format!(" AND context_type = '{}'", ct.replace('\'', "''")));
    }

    if let Some(ref ck) = req.context_key {
        query.push_str(&format!(" AND context_key = '{}'", ck.replace('\'', "''")));
    }

    if !include_expired {
        query.push_str(&format!(" AND (expires_at IS NULL OR expires_at > {})", now));
    }

    query.push_str(" ORDER BY updated_at DESC LIMIT 50");

    let rows = sqlx::query_as::<_, (String, String, String, Option<i64>, String)>(&query)
        .bind(project_id)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(context_type, context_key, context_value, expires_at, updated)| {
        let value: serde_json::Value = serde_json::from_str(&context_value)
            .unwrap_or(serde_json::Value::String(context_value));

        serde_json::json!({
            "context_type": context_type,
            "context_key": context_key,
            "value": value,
            "expires_at": expires_at,
            "updated": updated,
        })
    }).collect())
}

/// Get active todos from work state for session resume
pub async fn get_active_todos(
    db: &SqlitePool,
    project_id: Option<i64>,
) -> anyhow::Result<Option<Vec<WorkStateTodo>>> {
    let now = Utc::now().timestamp();

    let result = sqlx::query_as::<_, (String,)>(r#"
        SELECT context_value
        FROM work_context
        WHERE context_type = 'active_todos'
          AND (project_id IS NULL OR project_id = $1)
          AND (expires_at IS NULL OR expires_at > $2)
        ORDER BY updated_at DESC
        LIMIT 1
    "#)
    .bind(project_id)
    .bind(now)
    .fetch_optional(db)
    .await?;

    match result {
        Some((context_value,)) => {
            let todos: Vec<WorkStateTodo> = serde_json::from_str(&context_value)?;
            let pending: Vec<WorkStateTodo> = todos
                .into_iter()
                .filter(|t| t.status != "completed")
                .collect();

            if pending.is_empty() {
                Ok(None)
            } else {
                Ok(Some(pending))
            }
        }
        None => Ok(None),
    }
}

/// Get active plan from work state for session resume
pub async fn get_active_plan(
    db: &SqlitePool,
    project_id: Option<i64>,
) -> anyhow::Result<Option<ActivePlan>> {
    let now = Utc::now().timestamp();

    let result = sqlx::query_as::<_, (String,)>(r#"
        SELECT context_value
        FROM work_context
        WHERE context_type = 'active_plan'
          AND (project_id IS NULL OR project_id = $1)
          AND (expires_at IS NULL OR expires_at > $2)
        ORDER BY updated_at DESC
        LIMIT 1
    "#)
    .bind(project_id)
    .bind(now)
    .fetch_optional(db)
    .await?;

    match result {
        Some((context_value,)) => {
            let plan_json: serde_json::Value = serde_json::from_str(&context_value)?;

            let status = plan_json.get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let content = plan_json.get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let updated_at = plan_json.get("updated_at")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if status == "ready" && content.is_some() {
                Ok(Some(ActivePlan {
                    status,
                    content,
                    updated_at,
                }))
            } else if status == "planning" {
                Ok(Some(ActivePlan {
                    status,
                    content: None,
                    updated_at,
                }))
            } else {
                Ok(None)
            }
        }
        None => Ok(None),
    }
}

/// Get working documents from work state for session resume
pub async fn get_working_docs(
    db: &SqlitePool,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<WorkingDoc>> {
    let now = Utc::now().timestamp();

    let results = sqlx::query_as::<_, (String,)>(r#"
        SELECT context_value
        FROM work_context
        WHERE context_type = 'working_doc'
          AND (project_id IS NULL OR project_id = $1)
          AND (expires_at IS NULL OR expires_at > $2)
        ORDER BY updated_at DESC
        LIMIT 10
    "#)
    .bind(project_id)
    .bind(now)
    .fetch_all(db)
    .await?;

    let mut docs = Vec::new();

    for (context_value,) in results {
        if let Ok(doc_json) = serde_json::from_str::<serde_json::Value>(&context_value) {
            let filename = doc_json.get("filename")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let path = doc_json.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let content = doc_json.get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let preview: String = content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(3)
                .collect::<Vec<_>>()
                .join(" | ");

            let preview = if preview.len() > 100 {
                format!("{}...", &preview[..97])
            } else {
                preview
            };

            let updated_at = doc_json.get("updated_at")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            docs.push(WorkingDoc {
                filename,
                path,
                preview,
                updated_at,
            });
        }
    }

    Ok(docs)
}
