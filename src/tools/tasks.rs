// src/tools/tasks.rs
// Task management tools - thin wrapper delegating to core::ops::mira
//
// Keeps MCP-specific types separate from the shared core.

use sqlx::sqlite::SqlitePool;

use crate::core::ops::mira as core_mira;
use crate::core::OpContext;

// Parameter structs matching MCP request types
pub struct CreateTaskParams {
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub parent_id: Option<String>,
}

pub struct ListTasksParams {
    pub status: Option<String>,
    pub parent_id: Option<String>,
    pub include_completed: Option<bool>,
    pub limit: Option<i64>,
}

pub struct UpdateTaskParams {
    pub task_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
}

/// Create a new task
pub async fn create_task(
    db: &SqlitePool,
    req: CreateTaskParams,
) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::CreateTaskInput {
        title: req.title,
        description: req.description,
        priority: req.priority,
        parent_id: req.parent_id,
    };

    let output = core_mira::create_task(&ctx, input).await?;

    Ok(serde_json::json!({
        "status": "created",
        "task_id": output.task_id,
        "title": output.title,
        "priority": output.priority,
    }))
}

/// List tasks with optional filters
pub async fn list_tasks(
    db: &SqlitePool,
    req: ListTasksParams,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::ListTasksInput {
        status: req.status,
        parent_id: req.parent_id,
        include_completed: req.include_completed.unwrap_or(false),
        limit: req.limit.unwrap_or(20),
    };

    let tasks = core_mira::list_tasks(&ctx, input).await?;

    Ok(tasks
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "parent_id": t.parent_id,
                "title": t.title,
                "description": t.description,
                "status": t.status,
                "priority": t.priority,
                "project_path": t.project_path,
                "tags": t.tags,
                "created_at": t.created_at,
                "updated_at": t.updated_at,
                "completed_at": t.completed_at,
            })
        })
        .collect())
}

/// Get a specific task with its subtasks
pub async fn get_task(
    db: &SqlitePool,
    task_id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let task = core_mira::get_task(&ctx, task_id).await?;

    Ok(task.map(|t| {
        let subtasks: Vec<serde_json::Value> = t
            .subtasks
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "title": s.title,
                    "status": s.status,
                    "priority": s.priority,
                })
            })
            .collect();

        serde_json::json!({
            "id": t.id,
            "parent_id": t.parent_id,
            "title": t.title,
            "description": t.description,
            "status": t.status,
            "priority": t.priority,
            "project_path": t.project_path,
            "tags": t.tags,
            "completion_notes": t.completion_notes,
            "created_at": t.created_at,
            "updated_at": t.updated_at,
            "completed_at": t.completed_at,
            "subtasks": subtasks,
        })
    }))
}

/// Update an existing task
pub async fn update_task(
    db: &SqlitePool,
    req: UpdateTaskParams,
) -> anyhow::Result<Option<serde_json::Value>> {
    let task_id = req.task_id.clone();
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::UpdateTaskInput {
        task_id: req.task_id,
        title: req.title.clone(),
        description: req.description.clone(),
        status: req.status.clone(),
        priority: req.priority.clone(),
    };

    let updated = core_mira::update_task(&ctx, input).await?;

    if !updated {
        return Ok(None);
    }

    Ok(Some(serde_json::json!({
        "status": "updated",
        "task_id": task_id,
        "changes": {
            "title": req.title,
            "description": req.description,
            "status": req.status,
            "priority": req.priority,
        },
    })))
}

/// Mark a task as completed
pub async fn complete_task(
    db: &SqlitePool,
    task_id: &str,
    notes: Option<String>,
) -> anyhow::Result<Option<serde_json::Value>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let output = core_mira::complete_task(&ctx, task_id, notes).await?;

    Ok(output.map(|o| {
        serde_json::json!({
            "status": "completed",
            "task_id": o.task_id,
            "title": o.title,
            "completed_at": o.completed_at,
            "notes": o.notes,
        })
    }))
}

/// Delete a task and its subtasks
pub async fn delete_task(db: &SqlitePool, task_id: &str) -> anyhow::Result<Option<String>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    core_mira::delete_task(&ctx, task_id).await.map_err(Into::into)
}
