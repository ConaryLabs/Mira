//! Core work state operations - shared by MCP and Chat
//!
//! Work state management for seamless session resume.
//! Tracks transient state like active todos, plans, and working documents.

use chrono::Utc;

use super::super::{CoreResult, OpContext};

// ============================================================================
// Input/Output Types
// ============================================================================

pub struct SyncWorkStateInput {
    pub context_type: String,
    pub context_key: String,
    pub context_value: serde_json::Value,
    pub ttl_hours: i64,
    pub project_id: Option<i64>,
}

pub struct SyncWorkStateOutput {
    pub context_type: String,
    pub context_key: String,
    pub expires_at: i64,
}

pub struct GetWorkStateInput {
    pub context_type: Option<String>,
    pub context_key: Option<String>,
    pub include_expired: bool,
    pub project_id: Option<i64>,
}

pub struct WorkStateEntry {
    pub context_type: String,
    pub context_key: String,
    pub value: serde_json::Value,
    pub expires_at: Option<i64>,
    pub updated: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkStateTodo {
    pub content: String,
    pub status: String,
    #[serde(rename = "activeForm")]
    pub active_form: String,
}

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

// ============================================================================
// Operations
// ============================================================================

/// Sync work state for seamless session resume
pub async fn sync_work_state(ctx: &OpContext, input: SyncWorkStateInput) -> CoreResult<SyncWorkStateOutput> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let expires_at = now + (input.ttl_hours * 3600);

    let context_value = serde_json::to_string(&input.context_value)?;

    sqlx::query(r#"
        INSERT INTO work_context (context_type, context_key, context_value, priority, expires_at, created_at, updated_at, project_id)
        VALUES ($1, $2, $3, 0, $4, $5, $5, $6)
        ON CONFLICT(context_type, context_key) DO UPDATE SET
            context_value = excluded.context_value,
            expires_at = excluded.expires_at,
            updated_at = excluded.updated_at,
            project_id = COALESCE(excluded.project_id, work_context.project_id)
    "#)
    .bind(&input.context_type)
    .bind(&input.context_key)
    .bind(&context_value)
    .bind(expires_at)
    .bind(now)
    .bind(input.project_id)
    .execute(db)
    .await?;

    Ok(SyncWorkStateOutput {
        context_type: input.context_type,
        context_key: input.context_key,
        expires_at,
    })
}

/// Get work state for session resume
pub async fn get_work_state(ctx: &OpContext, input: GetWorkStateInput) -> CoreResult<Vec<WorkStateEntry>> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    let mut query = String::from(r#"
        SELECT context_type, context_key, context_value, expires_at,
               datetime(updated_at, 'unixepoch', 'localtime') as updated
        FROM work_context
        WHERE (project_id IS NULL OR project_id = $1)
    "#);

    if let Some(ref ct) = input.context_type {
        query.push_str(&format!(" AND context_type = '{}'", ct.replace('\'', "''")));
    }

    if let Some(ref ck) = input.context_key {
        query.push_str(&format!(" AND context_key = '{}'", ck.replace('\'', "''")));
    }

    if !input.include_expired {
        query.push_str(&format!(" AND (expires_at IS NULL OR expires_at > {})", now));
    }

    query.push_str(" ORDER BY updated_at DESC LIMIT 50");

    let rows = sqlx::query_as::<_, (String, String, String, Option<i64>, String)>(&query)
        .bind(input.project_id)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(context_type, context_key, context_value, expires_at, updated)| {
        let value: serde_json::Value = serde_json::from_str(&context_value)
            .unwrap_or(serde_json::Value::String(context_value));

        WorkStateEntry {
            context_type,
            context_key,
            value,
            expires_at,
            updated,
        }
    }).collect())
}

/// Get active todos from work state for session resume
pub async fn get_active_todos(ctx: &OpContext, project_id: Option<i64>) -> CoreResult<Option<Vec<WorkStateTodo>>> {
    let db = ctx.require_db()?;
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
pub async fn get_active_plan(ctx: &OpContext, project_id: Option<i64>) -> CoreResult<Option<ActivePlan>> {
    let db = ctx.require_db()?;
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
pub async fn get_working_docs(ctx: &OpContext, project_id: Option<i64>) -> CoreResult<Vec<WorkingDoc>> {
    let db = ctx.require_db()?;
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
