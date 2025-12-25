//! Task operations - create, list, get, update, complete, delete

use crate::core::{CoreError, CoreResult, OpContext};
use chrono::Utc;
use uuid::Uuid;

/// Input for creating a task
#[derive(Debug, Clone, Default)]
pub struct CreateTaskInput {
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub parent_id: Option<String>,
}

/// Input for listing tasks
#[derive(Debug, Clone, Default)]
pub struct ListTasksInput {
    pub status: Option<String>,
    pub parent_id: Option<String>,
    pub include_completed: bool,
    pub limit: i64,
}

/// Input for updating a task
#[derive(Debug, Clone, Default)]
pub struct UpdateTaskInput {
    pub task_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
}

/// Task data returned from operations
#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub project_path: Option<String>,
    pub tags: Option<String>,
    pub completion_notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub subtasks: Vec<TaskSummary>,
}

/// Brief task info for subtask lists
#[derive(Debug, Clone)]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
}

/// Result of task creation
#[derive(Debug, Clone)]
pub struct CreateTaskOutput {
    pub task_id: String,
    pub title: String,
    pub priority: String,
}

/// Result of completing a task
#[derive(Debug, Clone)]
pub struct CompleteTaskOutput {
    pub task_id: String,
    pub title: String,
    pub completed_at: String,
    pub notes: Option<String>,
}

/// Resolve a task ID - supports both full UUIDs and short prefixes
async fn resolve_task_id(ctx: &OpContext, task_id: &str) -> CoreResult<Option<String>> {
    let db = ctx.require_db()?;

    // If it looks like a full UUID, use directly
    if task_id.len() == 36 && task_id.contains('-') {
        return Ok(Some(task_id.to_string()));
    }

    // Otherwise, treat as prefix and find matching task
    let pattern = format!("{}%", task_id);
    let result = sqlx::query_scalar::<_, String>("SELECT id FROM tasks WHERE id LIKE $1 LIMIT 1")
        .bind(&pattern)
        .fetch_optional(db)
        .await?;

    Ok(result)
}

/// Create a new task
pub async fn create_task(ctx: &OpContext, input: CreateTaskInput) -> CoreResult<CreateTaskOutput> {
    if input.title.is_empty() {
        return Err(CoreError::MissingField("title"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let priority = input.priority.as_deref().unwrap_or("medium");

    sqlx::query(
        r#"
        INSERT INTO tasks (id, parent_id, title, description, status, priority, created_at, updated_at)
        VALUES ($1, $2, $3, $4, 'pending', $5, $6, $6)
        "#,
    )
    .bind(&id)
    .bind(&input.parent_id)
    .bind(&input.title)
    .bind(&input.description)
    .bind(priority)
    .bind(now)
    .execute(db)
    .await?;

    Ok(CreateTaskOutput {
        task_id: id,
        title: input.title,
        priority: priority.to_string(),
    })
}

/// List tasks with optional filters
pub async fn list_tasks(ctx: &OpContext, input: ListTasksInput) -> CoreResult<Vec<Task>> {
    let db = ctx.require_db()?;

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

    let rows = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
            Option<String>,
        ),
    >(query)
    .bind(&input.status)
    .bind(&input.parent_id)
    .bind(if input.include_completed { 1 } else { 0 })
    .bind(input.limit)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                parent_id,
                title,
                desc,
                status,
                priority,
                project_path,
                tags,
                created,
                updated,
                completed,
            )| {
                Task {
                    id,
                    parent_id,
                    title,
                    description: desc,
                    status,
                    priority,
                    project_path,
                    tags,
                    completion_notes: None,
                    created_at: created,
                    updated_at: updated,
                    completed_at: completed,
                    subtasks: vec![],
                }
            },
        )
        .collect())
}

/// Get a specific task with its subtasks
pub async fn get_task(ctx: &OpContext, task_id: &str) -> CoreResult<Option<Task>> {
    let db = ctx.require_db()?;

    let full_id = match resolve_task_id(ctx, task_id).await? {
        Some(id) => id,
        None => return Ok(None),
    };

    let task_query = r#"
        SELECT id, parent_id, title, description, status, priority, project_path, tags,
               completion_notes,
               datetime(created_at, 'unixepoch', 'localtime') as created_at,
               datetime(updated_at, 'unixepoch', 'localtime') as updated_at,
               datetime(completed_at, 'unixepoch', 'localtime') as completed_at
        FROM tasks
        WHERE id = $1
    "#;

    let task = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            String,
            Option<String>,
        ),
    >(task_query)
    .bind(&full_id)
    .fetch_optional(db)
    .await?;

    match task {
        Some((
            id,
            parent_id,
            title,
            desc,
            status,
            priority,
            project_path,
            tags,
            completion_notes,
            created,
            updated,
            completed,
        )) => {
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
                .bind(&full_id)
                .fetch_all(db)
                .await
                .unwrap_or_default();

            let subtask_list: Vec<TaskSummary> = subtasks
                .into_iter()
                .map(|(id, title, status, priority)| TaskSummary {
                    id,
                    title,
                    status,
                    priority,
                })
                .collect();

            Ok(Some(Task {
                id,
                parent_id,
                title,
                description: desc,
                status,
                priority,
                project_path,
                tags,
                completion_notes,
                created_at: created,
                updated_at: updated,
                completed_at: completed,
                subtasks: subtask_list,
            }))
        }
        None => Ok(None),
    }
}

/// Update an existing task
pub async fn update_task(ctx: &OpContext, input: UpdateTaskInput) -> CoreResult<bool> {
    if input.task_id.is_empty() {
        return Err(CoreError::MissingField("task_id"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    let full_id = match resolve_task_id(ctx, &input.task_id).await? {
        Some(id) => id,
        None => return Ok(false),
    };

    let result = sqlx::query(
        r#"
        UPDATE tasks
        SET updated_at = $1,
            title = COALESCE($2, title),
            description = COALESCE($3, description),
            status = COALESCE($4, status),
            priority = COALESCE($5, priority)
        WHERE id = $6
        "#,
    )
    .bind(now)
    .bind(&input.title)
    .bind(&input.description)
    .bind(&input.status)
    .bind(&input.priority)
    .bind(&full_id)
    .execute(db)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Mark a task as completed
pub async fn complete_task(
    ctx: &OpContext,
    task_id: &str,
    notes: Option<String>,
) -> CoreResult<Option<CompleteTaskOutput>> {
    if task_id.is_empty() {
        return Err(CoreError::MissingField("task_id"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    let full_id = match resolve_task_id(ctx, task_id).await? {
        Some(id) => id,
        None => return Ok(None),
    };

    let result = sqlx::query(
        r#"
        UPDATE tasks
        SET status = 'completed',
            completed_at = $1,
            updated_at = $1,
            completion_notes = $2
        WHERE id = $3
        "#,
    )
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

    Ok(Some(CompleteTaskOutput {
        task_id: full_id,
        title,
        completed_at: Utc::now().to_rfc3339(),
        notes,
    }))
}

/// Delete a task and its subtasks
pub async fn delete_task(ctx: &OpContext, task_id: &str) -> CoreResult<Option<String>> {
    if task_id.is_empty() {
        return Err(CoreError::MissingField("task_id"));
    }

    let db = ctx.require_db()?;

    let full_id = match resolve_task_id(ctx, task_id).await? {
        Some(id) => id,
        None => return Ok(None),
    };

    let task = sqlx::query_as::<_, (String,)>("SELECT title FROM tasks WHERE id = $1")
        .bind(&full_id)
        .fetch_optional(db)
        .await?;

    match task {
        Some((title,)) => {
            // Delete subtasks first
            sqlx::query("DELETE FROM tasks WHERE parent_id = $1")
                .bind(&full_id)
                .execute(db)
                .await?;

            // Delete the task itself
            sqlx::query("DELETE FROM tasks WHERE id = $1")
                .bind(&full_id)
                .execute(db)
                .await?;

            Ok(Some(title))
        }
        None => Ok(None),
    }
}
