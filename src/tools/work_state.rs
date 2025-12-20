// src/tools/work_state.rs
// Work state management - thin wrapper over core::ops::work_state

use sqlx::sqlite::SqlitePool;

use crate::core::ops::work_state as core_work;
use crate::core::OpContext;
use super::types::{SyncWorkStateRequest, GetWorkStateRequest};

// Re-export types from core
pub use core_work::{ActivePlan, WorkingDoc, WorkStateTodo};

/// Sync work state for seamless session resume
pub async fn sync_work_state(
    db: &SqlitePool,
    req: SyncWorkStateRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::just_db(db.clone());

    let input = core_work::SyncWorkStateInput {
        context_type: req.context_type,
        context_key: req.context_key,
        context_value: req.context_value,
        ttl_hours: req.ttl_hours.unwrap_or(24),
        project_id,
    };

    let output = core_work::sync_work_state(&ctx, input).await?;

    Ok(serde_json::json!({
        "status": "synced",
        "context_type": output.context_type,
        "context_key": output.context_key,
        "expires_at": output.expires_at,
    }))
}

/// Get work state for session resume
pub async fn get_work_state(
    db: &SqlitePool,
    req: GetWorkStateRequest,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::just_db(db.clone());

    let input = core_work::GetWorkStateInput {
        context_type: req.context_type,
        context_key: req.context_key,
        include_expired: req.include_expired.unwrap_or(false),
        project_id,
    };

    let entries = core_work::get_work_state(&ctx, input).await?;

    Ok(entries.into_iter().map(|e| {
        serde_json::json!({
            "context_type": e.context_type,
            "context_key": e.context_key,
            "value": e.value,
            "expires_at": e.expires_at,
            "updated": e.updated,
        })
    }).collect())
}

/// Get active todos from work state for session resume
pub async fn get_active_todos(
    db: &SqlitePool,
    project_id: Option<i64>,
) -> anyhow::Result<Option<Vec<WorkStateTodo>>> {
    let ctx = OpContext::just_db(db.clone());
    core_work::get_active_todos(&ctx, project_id).await.map_err(Into::into)
}

/// Get active plan from work state for session resume
pub async fn get_active_plan(
    db: &SqlitePool,
    project_id: Option<i64>,
) -> anyhow::Result<Option<ActivePlan>> {
    let ctx = OpContext::just_db(db.clone());
    core_work::get_active_plan(&ctx, project_id).await.map_err(Into::into)
}

/// Get working documents from work state for session resume
pub async fn get_working_docs(
    db: &SqlitePool,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<WorkingDoc>> {
    let ctx = OpContext::just_db(db.clone());
    core_work::get_working_docs(&ctx, project_id).await.map_err(Into::into)
}
