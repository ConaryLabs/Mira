// src/tools/tasks.rs
// Task management tools - persistent tasks across sessions

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

/// Resolve a task ID - supports both full UUIDs and short prefixes
async fn resolve_task_id(db: &SqlitePool, task_id: &str) -> anyhow::Result<Option<String>> {
    // If it looks like a full UUID, use directly
    if task_id.len() == 36 && task_id.contains('-') {
        return Ok(Some(task_id.to_string()));
    }

    // Otherwise, treat as prefix and find matching task
    let pattern = format!("{}%", task_id);
    let result = sqlx::query_scalar::<_, String>(
        "SELECT id FROM tasks WHERE id LIKE $1 LIMIT 1"
    )
    .bind(&pattern)
    .fetch_optional(db)
    .await?;

    Ok(result)
}

// === Parameter structs for consolidated task tool ===

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
pub async fn create_task(db: &SqlitePool, req: CreateTaskParams) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let priority = req.priority.as_deref().unwrap_or("medium");

    let result = sqlx::query(r#"
        INSERT INTO tasks (id, parent_id, title, description, status, priority, created_at, updated_at)
        VALUES ($1, $2, $3, $4, 'pending', $5, $6, $6)
    "#)
    .bind(&id)
    .bind(&req.parent_id)
    .bind(&req.title)
    .bind(&req.description)
    .bind(priority)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "created",
        "task_id": id,
        "title": req.title,
        "priority": priority,
        "rows_affected": result.rows_affected(),
    }))
}

/// List tasks with optional filters
pub async fn list_tasks(db: &SqlitePool, req: ListTasksParams) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(20);
    let include_completed = req.include_completed.unwrap_or(false);

    let query = r#"
        SELECT id, parent_id, title, description, status, priority, project_path, tags,
               datetime(created_at, 'unixepoch', 'localtime') as created_at,
               datetime(updated_at, 'unixepoch', 'localtime') as updated_at,
               datetime(completed_at, 'unixepoch', 'localtime') as completed_at
        FROM tasks
        WHERE ($1 IS NULL OR status = $1)
          AND ($2 IS NULL OR parent_id = $2)
          AND ($3 = 1 OR status != 'completed')
        ORDER BY
            CASE status
                WHEN 'in_progress' THEN 0
                WHEN 'blocked' THEN 1
                WHEN 'pending' THEN 2
                ELSE 3
            END,
            CASE priority
                WHEN 'urgent' THEN 0
                WHEN 'high' THEN 1
                WHEN 'medium' THEN 2
                ELSE 3
            END,
            created_at DESC
        LIMIT $4
    "#;

    let rows = sqlx::query_as::<_, (String, Option<String>, String, Option<String>, String, String, Option<String>, Option<String>, String, String, Option<String>)>(query)
        .bind(&req.status)
        .bind(&req.parent_id)
        .bind(if include_completed { 1 } else { 0 })
        .bind(limit)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, parent_id, title, desc, status, priority, project_path, tags, created, updated, completed)| {
            serde_json::json!({
                "id": id,
                "parent_id": parent_id,
                "title": title,
                "description": desc,
                "status": status,
                "priority": priority,
                "project_path": project_path,
                "tags": tags,
                "created_at": created,
                "updated_at": updated,
                "completed_at": completed,
            })
        })
        .collect())
}

/// Get a specific task with its subtasks
pub async fn get_task(db: &SqlitePool, task_id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    let task_query = r#"
        SELECT id, parent_id, title, description, status, priority, project_path, tags,
               completion_notes,
               datetime(created_at, 'unixepoch', 'localtime') as created_at,
               datetime(updated_at, 'unixepoch', 'localtime') as updated_at,
               datetime(completed_at, 'unixepoch', 'localtime') as completed_at
        FROM tasks
        WHERE id = $1
    "#;

    let task = sqlx::query_as::<_, (String, Option<String>, String, Option<String>, String, String, Option<String>, Option<String>, Option<String>, String, String, Option<String>)>(task_query)
        .bind(task_id)
        .fetch_optional(db)
        .await?;

    match task {
        Some((id, parent_id, title, desc, status, priority, project_path, tags, completion_notes, created, updated, completed)) => {
            // Get subtasks
            let subtasks_query = r#"
                SELECT id, title, status, priority
                FROM tasks
                WHERE parent_id = $1
                ORDER BY
                    CASE status WHEN 'in_progress' THEN 0 WHEN 'pending' THEN 1 ELSE 2 END,
                    created_at
            "#;

            let subtasks = sqlx::query_as::<_, (String, String, String, String)>(subtasks_query)
                .bind(task_id)
                .fetch_all(db)
                .await
                .unwrap_or_default();

            let subtask_list: Vec<serde_json::Value> = subtasks
                .into_iter()
                .map(|(id, title, status, priority)| {
                    serde_json::json!({
                        "id": id,
                        "title": title,
                        "status": status,
                        "priority": priority,
                    })
                })
                .collect();

            Ok(Some(serde_json::json!({
                "id": id,
                "parent_id": parent_id,
                "title": title,
                "description": desc,
                "status": status,
                "priority": priority,
                "project_path": project_path,
                "tags": tags,
                "completion_notes": completion_notes,
                "created_at": created,
                "updated_at": updated,
                "completed_at": completed,
                "subtasks": subtask_list,
            })))
        }
        None => Ok(None),
    }
}

/// Update an existing task
pub async fn update_task(db: &SqlitePool, req: UpdateTaskParams) -> anyhow::Result<Option<serde_json::Value>> {
    let now = Utc::now().timestamp();

    let result = sqlx::query(r#"
        UPDATE tasks
        SET updated_at = $1,
            title = COALESCE($2, title),
            description = COALESCE($3, description),
            status = COALESCE($4, status),
            priority = COALESCE($5, priority)
        WHERE id = $6
    "#)
    .bind(now)
    .bind(&req.title)
    .bind(&req.description)
    .bind(&req.status)
    .bind(&req.priority)
    .bind(&req.task_id)
    .execute(db)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    Ok(Some(serde_json::json!({
        "status": "updated",
        "task_id": req.task_id,
        "changes": {
            "title": req.title,
            "description": req.description,
            "status": req.status,
            "priority": req.priority,
        },
    })))
}

/// Mark a task as completed
/// Supports both full UUIDs and short ID prefixes (e.g., "3ec77d3f")
pub async fn complete_task(db: &SqlitePool, task_id: &str, notes: Option<String>) -> anyhow::Result<Option<serde_json::Value>> {
    let now = Utc::now().timestamp();

    // Support short ID prefixes by finding the full ID first
    let full_id = resolve_task_id(db, task_id).await?;
    let full_id = match full_id {
        Some(id) => id,
        None => return Ok(None),
    };

    let result = sqlx::query(r#"
        UPDATE tasks
        SET status = 'completed',
            completed_at = $1,
            updated_at = $1,
            completion_notes = $2
        WHERE id = $3
    "#)
    .bind(now)
    .bind(&notes)
    .bind(&full_id)
    .execute(db)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    // Get the title for the response
    let title = sqlx::query_scalar::<_, String>("SELECT title FROM tasks WHERE id = $1")
        .bind(&full_id)
        .fetch_optional(db)
        .await?
        .unwrap_or_else(|| "?".to_string());

    Ok(Some(serde_json::json!({
        "status": "completed",
        "task_id": full_id,
        "title": title,
        "completed_at": Utc::now().to_rfc3339(),
        "notes": notes,
    })))
}

/// Delete a task and its subtasks
pub async fn delete_task(db: &SqlitePool, task_id: &str) -> anyhow::Result<Option<String>> {
    let task = sqlx::query_as::<_, (String,)>("SELECT title FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_optional(db)
        .await?;

    match task {
        Some((title,)) => {
            // Delete subtasks first
            sqlx::query("DELETE FROM tasks WHERE parent_id = $1")
                .bind(task_id)
                .execute(db)
                .await?;

            // Delete the task itself
            sqlx::query("DELETE FROM tasks WHERE id = $1")
                .bind(task_id)
                .execute(db)
                .await?;

            Ok(Some(title))
        }
        None => Ok(None),
    }
}
