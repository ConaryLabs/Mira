//! Mira power armor operations - task, goal, correction, decision, rejected_approach
//!
//! Unified implementation for MCP and Chat tools.
//! Database operations for project management and learning from corrections.

use crate::core::{CoreError, CoreResult, OpContext};
use chrono::Utc;
use uuid::Uuid;

// ============================================================================
// Task Operations
// ============================================================================

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

/// Result of completing a task
#[derive(Debug, Clone)]
pub struct CompleteTaskOutput {
    pub task_id: String,
    pub title: String,
    pub completed_at: String,
    pub notes: Option<String>,
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

// ============================================================================
// Goal Operations
// ============================================================================

/// Input for creating a goal
#[derive(Debug, Clone, Default)]
pub struct CreateGoalInput {
    pub title: String,
    pub description: Option<String>,
    pub success_criteria: Option<String>,
    pub priority: Option<String>,
    pub project_id: Option<i64>,
}

/// Input for listing goals
#[derive(Debug, Clone, Default)]
pub struct ListGoalsInput {
    pub status: Option<String>,
    pub include_finished: bool,
    pub limit: i64,
    pub project_id: Option<i64>,
}

/// Input for updating a goal
#[derive(Debug, Clone, Default)]
pub struct UpdateGoalInput {
    pub goal_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub progress_percent: Option<i32>,
}

/// Input for adding a milestone
#[derive(Debug, Clone)]
pub struct AddMilestoneInput {
    pub goal_id: String,
    pub title: String,
    pub description: Option<String>,
    pub weight: Option<i32>,
}

/// Goal summary for lists
#[derive(Debug, Clone)]
pub struct GoalSummary {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub progress_percent: i32,
    pub milestones_completed: i64,
    pub milestones_total: i64,
    pub has_blockers: bool,
    pub updated_at: String,
}

/// Result of goal creation
#[derive(Debug, Clone)]
pub struct CreateGoalOutput {
    pub goal_id: String,
    pub title: String,
    pub priority: String,
}

/// Create a new goal
pub async fn create_goal(ctx: &OpContext, input: CreateGoalInput) -> CoreResult<CreateGoalOutput> {
    if input.title.is_empty() {
        return Err(CoreError::MissingField("title"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let id = format!(
        "goal-{}",
        Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let priority = input.priority.as_deref().unwrap_or("medium");

    sqlx::query(
        r#"
        INSERT INTO goals (id, title, description, success_criteria, status, priority,
                          project_id, created_at, updated_at)
        VALUES ($1, $2, $3, $4, 'planning', $5, $6, $7, $7)
        "#,
    )
    .bind(&id)
    .bind(&input.title)
    .bind(&input.description)
    .bind(&input.success_criteria)
    .bind(priority)
    .bind(input.project_id)
    .bind(now)
    .execute(db)
    .await?;

    Ok(CreateGoalOutput {
        goal_id: id,
        title: input.title,
        priority: priority.to_string(),
    })
}

/// List goals with optional filters
pub async fn list_goals(ctx: &OpContext, input: ListGoalsInput) -> CoreResult<Vec<GoalSummary>> {
    let db = ctx.require_db()?;

    let base_query = if input.include_finished {
        r#"
            SELECT id, title, description, status, priority, progress_percent, progress_mode, blockers,
                   datetime(updated_at, 'unixepoch', 'localtime') as updated
            FROM goals
            WHERE (project_id IS NULL OR project_id = $1)
              AND ($2 IS NULL OR status = $2)
            ORDER BY
                CASE status WHEN 'blocked' THEN 1 WHEN 'in_progress' THEN 2 WHEN 'planning' THEN 3 ELSE 4 END,
                CASE priority WHEN 'critical' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END,
                updated_at DESC
            LIMIT $3
        "#
    } else {
        r#"
            SELECT id, title, description, status, priority, progress_percent, progress_mode, blockers,
                   datetime(updated_at, 'unixepoch', 'localtime') as updated
            FROM goals
            WHERE (project_id IS NULL OR project_id = $1)
              AND status IN ('planning', 'in_progress', 'blocked')
              AND ($2 IS NULL OR status = $2)
            ORDER BY
                CASE status WHEN 'blocked' THEN 1 WHEN 'in_progress' THEN 2 WHEN 'planning' THEN 3 END,
                CASE priority WHEN 'critical' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END,
                updated_at DESC
            LIMIT $3
        "#
    };

    let results = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            String,
            String,
            i32,
            String,
            Option<String>,
            String,
        ),
    >(base_query)
    .bind(input.project_id)
    .bind(&input.status)
    .bind(input.limit)
    .fetch_all(db)
    .await?;

    let mut goals = Vec::new();
    for (id, title, description, status, priority, progress, progress_mode, blockers, updated) in
        results
    {
        let actual_progress = if progress_mode == "auto" {
            calculate_goal_progress(db, &id).await.unwrap_or(progress)
        } else {
            progress
        };
        let (total, completed) = get_milestone_counts(db, &id).await.unwrap_or((0, 0));

        goals.push(GoalSummary {
            id,
            title,
            description,
            status,
            priority,
            progress_percent: actual_progress,
            milestones_completed: completed,
            milestones_total: total,
            has_blockers: blockers.map(|b| !b.is_empty()).unwrap_or(false),
            updated_at: updated,
        });
    }

    Ok(goals)
}

/// Update a goal
pub async fn update_goal(ctx: &OpContext, input: UpdateGoalInput) -> CoreResult<bool> {
    if input.goal_id.is_empty() {
        return Err(CoreError::MissingField("goal_id"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM goals WHERE id = $1")
        .bind(&input.goal_id)
        .fetch_one(db)
        .await?
        > 0;

    if !exists {
        return Ok(false);
    }

    if let Some(title) = &input.title {
        sqlx::query("UPDATE goals SET title = $2, updated_at = $3 WHERE id = $1")
            .bind(&input.goal_id)
            .bind(title)
            .bind(now)
            .execute(db)
            .await?;
    }

    if let Some(description) = &input.description {
        sqlx::query("UPDATE goals SET description = $2, updated_at = $3 WHERE id = $1")
            .bind(&input.goal_id)
            .bind(description)
            .bind(now)
            .execute(db)
            .await?;
    }

    if let Some(status) = &input.status {
        let completed_at = if status == "completed" {
            Some(now)
        } else {
            None
        };
        let started_at = if status == "in_progress" {
            Some(now)
        } else {
            None
        };

        sqlx::query(
            r#"
            UPDATE goals
            SET status = $2,
                completed_at = COALESCE($3, completed_at),
                started_at = COALESCE($4, started_at),
                updated_at = $5
            WHERE id = $1
            "#,
        )
        .bind(&input.goal_id)
        .bind(status)
        .bind(completed_at)
        .bind(started_at)
        .bind(now)
        .execute(db)
        .await?;
    }

    if let Some(priority) = &input.priority {
        sqlx::query("UPDATE goals SET priority = $2, updated_at = $3 WHERE id = $1")
            .bind(&input.goal_id)
            .bind(priority)
            .bind(now)
            .execute(db)
            .await?;
    }

    if let Some(progress) = input.progress_percent {
        sqlx::query(
            "UPDATE goals SET progress_percent = $2, progress_mode = 'manual', updated_at = $3 WHERE id = $1",
        )
        .bind(&input.goal_id)
        .bind(progress)
        .bind(now)
        .execute(db)
        .await?;
    }

    Ok(true)
}

/// Result of adding a milestone
#[derive(Debug, Clone)]
pub struct AddMilestoneOutput {
    pub milestone_id: String,
    pub goal_id: String,
    pub title: String,
}

/// Add a milestone to a goal
pub async fn add_milestone(
    ctx: &OpContext,
    input: AddMilestoneInput,
) -> CoreResult<AddMilestoneOutput> {
    if input.goal_id.is_empty() {
        return Err(CoreError::MissingField("goal_id"));
    }
    if input.title.is_empty() {
        return Err(CoreError::MissingField("title"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let id = format!(
        "ms-{}",
        Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let weight = input.weight.unwrap_or(1);

    let max_order: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(order_index), -1) FROM milestones WHERE goal_id = $1",
    )
    .bind(&input.goal_id)
    .fetch_one(db)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO milestones (id, goal_id, title, description, weight, order_index, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
        "#,
    )
    .bind(&id)
    .bind(&input.goal_id)
    .bind(&input.title)
    .bind(&input.description)
    .bind(weight)
    .bind(max_order + 1)
    .bind(now)
    .execute(db)
    .await?;

    sqlx::query("UPDATE goals SET updated_at = $2 WHERE id = $1")
        .bind(&input.goal_id)
        .bind(now)
        .execute(db)
        .await?;

    Ok(AddMilestoneOutput {
        milestone_id: id,
        goal_id: input.goal_id,
        title: input.title,
    })
}

/// Result of completing a milestone
#[derive(Debug, Clone)]
pub struct CompleteMilestoneOutput {
    pub milestone_id: String,
    pub goal_id: String,
    pub goal_progress_percent: i32,
    pub milestones_completed: i64,
    pub milestones_total: i64,
    pub all_milestones_complete: bool,
}

/// Complete a milestone
pub async fn complete_milestone(
    ctx: &OpContext,
    milestone_id: &str,
) -> CoreResult<Option<CompleteMilestoneOutput>> {
    if milestone_id.is_empty() {
        return Err(CoreError::MissingField("milestone_id"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    let milestone =
        sqlx::query_as::<_, (String, String)>("SELECT id, goal_id FROM milestones WHERE id = $1")
            .bind(milestone_id)
            .fetch_optional(db)
            .await?;

    let (_, goal_id) = match milestone {
        Some(m) => m,
        None => return Ok(None),
    };

    sqlx::query(
        r#"
        UPDATE milestones
        SET status = 'completed', completed_at = $2, updated_at = $2
        WHERE id = $1
        "#,
    )
    .bind(milestone_id)
    .bind(now)
    .execute(db)
    .await?;

    let new_progress = calculate_goal_progress(db, &goal_id).await?;

    sqlx::query(
        "UPDATE goals SET progress_percent = $2, updated_at = $3 WHERE id = $1 AND progress_mode = 'auto'",
    )
    .bind(&goal_id)
    .bind(new_progress)
    .bind(now)
    .execute(db)
    .await?;

    let (total, completed) = get_milestone_counts(db, &goal_id).await?;

    Ok(Some(CompleteMilestoneOutput {
        milestone_id: milestone_id.to_string(),
        goal_id,
        goal_progress_percent: new_progress,
        milestones_completed: completed,
        milestones_total: total,
        all_milestones_complete: completed == total,
    }))
}

// Helper functions for goals
async fn calculate_goal_progress(
    db: &sqlx::SqlitePool,
    goal_id: &str,
) -> CoreResult<i32> {
    let milestones =
        sqlx::query_as::<_, (i32, String)>("SELECT weight, status FROM milestones WHERE goal_id = $1")
            .bind(goal_id)
            .fetch_all(db)
            .await?;

    if milestones.is_empty() {
        return Ok(0);
    }

    let total_weight: i32 = milestones.iter().map(|(w, _)| *w).sum();
    let completed_weight: i32 = milestones
        .iter()
        .filter(|(_, s)| s == "completed")
        .map(|(w, _)| *w)
        .sum();

    if total_weight > 0 {
        Ok(((completed_weight as f64 / total_weight as f64) * 100.0).round() as i32)
    } else {
        Ok(0)
    }
}

async fn get_milestone_counts(
    db: &sqlx::SqlitePool,
    goal_id: &str,
) -> CoreResult<(i64, i64)> {
    let counts = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT
            COUNT(*) as total,
            SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed
        FROM milestones WHERE goal_id = $1
        "#,
    )
    .bind(goal_id)
    .fetch_one(db)
    .await?;

    Ok(counts)
}

// ============================================================================
// Correction Operations
// ============================================================================

/// Input for recording a correction
#[derive(Debug, Clone)]
pub struct RecordCorrectionInput {
    pub correction_type: String,
    pub what_was_wrong: String,
    pub what_is_right: String,
    pub rationale: Option<String>,
    pub scope: Option<String>,
    pub keywords: Option<String>,
    pub project_id: Option<i64>,
}

/// Input for listing corrections
#[derive(Debug, Clone, Default)]
pub struct ListCorrectionsInput {
    pub correction_type: Option<String>,
    pub scope: Option<String>,
    pub status: Option<String>,
    pub limit: i64,
    pub project_id: Option<i64>,
}

/// Correction data
#[derive(Debug, Clone)]
pub struct Correction {
    pub id: String,
    pub correction_type: String,
    pub what_was_wrong: String,
    pub what_is_right: String,
    pub rationale: Option<String>,
    pub scope: String,
    pub confidence: f64,
    pub times_applied: i64,
    pub times_validated: i64,
    pub created_at: Option<String>,
}

/// Result of recording a correction
#[derive(Debug, Clone)]
pub struct RecordCorrectionOutput {
    pub correction_id: String,
    pub correction_type: String,
    pub scope: String,
}

/// Record a new correction
pub async fn record_correction(
    ctx: &OpContext,
    input: RecordCorrectionInput,
) -> CoreResult<RecordCorrectionOutput> {
    if input.what_was_wrong.is_empty() {
        return Err(CoreError::MissingField("what_was_wrong"));
    }
    if input.what_is_right.is_empty() {
        return Err(CoreError::MissingField("what_is_right"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let scope = input.scope.as_deref().unwrap_or("project");
    let keywords = normalize_json_array(&input.keywords);

    sqlx::query(
        r#"
        INSERT INTO corrections (id, correction_type, what_was_wrong, what_is_right, rationale,
                                scope, project_id, keywords, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
        "#,
    )
    .bind(&id)
    .bind(&input.correction_type)
    .bind(&input.what_was_wrong)
    .bind(&input.what_is_right)
    .bind(&input.rationale)
    .bind(scope)
    .bind(if scope == "global" {
        None
    } else {
        input.project_id
    })
    .bind(&keywords)
    .bind(now)
    .execute(db)
    .await?;

    // Store in semantic search for fuzzy matching
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            use mira_core::semantic::COLLECTION_CONVERSATION;
            use mira_core::semantic_helpers::{store_with_logging, MetadataBuilder};

            let content = format!(
                "Correction: {} -> {}. Rationale: {}",
                input.what_was_wrong,
                input.what_is_right,
                input.rationale.as_deref().unwrap_or("")
            );
            let metadata = MetadataBuilder::new("correction")
                .string("correction_type", &input.correction_type)
                .string("scope", scope)
                .string("id", &id)
                .project_id(input.project_id)
                .build();
            store_with_logging(semantic, COLLECTION_CONVERSATION, &id, &content, metadata).await;
        }
    }

    Ok(RecordCorrectionOutput {
        correction_id: id,
        correction_type: input.correction_type,
        scope: scope.to_string(),
    })
}

/// List corrections
pub async fn list_corrections(
    ctx: &OpContext,
    input: ListCorrectionsInput,
) -> CoreResult<Vec<Correction>> {
    let db = ctx.require_db()?;
    let status = input.status.as_deref().unwrap_or("active");

    let results = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            f64,
            i64,
            i64,
            String,
        ),
    >(
        r#"
        SELECT id, correction_type, what_was_wrong, what_is_right, rationale, scope,
               confidence, times_applied, times_validated,
               datetime(created_at, 'unixepoch', 'localtime') as created
        FROM corrections
        WHERE status = $1
          AND (project_id IS NULL OR project_id = $2)
          AND ($3 IS NULL OR correction_type = $3)
          AND ($4 IS NULL OR scope = $4)
        ORDER BY created_at DESC
        LIMIT $5
        "#,
    )
    .bind(status)
    .bind(input.project_id)
    .bind(&input.correction_type)
    .bind(&input.scope)
    .bind(input.limit)
    .fetch_all(db)
    .await?;

    Ok(results
        .into_iter()
        .map(
            |(id, ctype, wrong, right, rationale, scope, confidence, applied, validated, created)| {
                Correction {
                    id,
                    correction_type: ctype,
                    what_was_wrong: wrong,
                    what_is_right: right,
                    rationale,
                    scope,
                    confidence,
                    times_applied: applied,
                    times_validated: validated,
                    created_at: Some(created),
                }
            },
        )
        .collect())
}

/// Validate a correction
pub async fn validate_correction(
    ctx: &OpContext,
    correction_id: &str,
    outcome: &str,
) -> CoreResult<bool> {
    if correction_id.is_empty() {
        return Err(CoreError::MissingField("correction_id"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    match outcome {
        "validated" => {
            sqlx::query(
                r#"
                UPDATE corrections
                SET times_validated = times_validated + 1,
                    confidence = MIN(1.0, confidence + 0.05),
                    updated_at = $2
                WHERE id = $1
                "#,
            )
            .bind(correction_id)
            .bind(now)
            .execute(db)
            .await?;
        }
        "overridden" => {
            sqlx::query("UPDATE corrections SET updated_at = $2 WHERE id = $1")
                .bind(correction_id)
                .bind(now)
                .execute(db)
                .await?;
        }
        "deprecated" => {
            sqlx::query(
                r#"
                UPDATE corrections
                SET status = 'deprecated', updated_at = $2
                WHERE id = $1
                "#,
            )
            .bind(correction_id)
            .bind(now)
            .execute(db)
            .await?;
        }
        _ => {
            return Err(CoreError::InvalidArgument(format!(
                "Invalid outcome: {}. Use 'validated', 'overridden', or 'deprecated'",
                outcome
            )));
        }
    }

    sqlx::query(
        r#"
        INSERT INTO correction_applications (correction_id, outcome, applied_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(correction_id)
    .bind(outcome)
    .bind(now)
    .execute(db)
    .await?;

    Ok(true)
}

// ============================================================================
// Decision Operations
// ============================================================================

/// Input for storing a decision
#[derive(Debug, Clone)]
pub struct StoreDecisionInput {
    pub key: String,
    pub decision: String,
    pub category: Option<String>,
    pub context: Option<String>,
    pub project_id: Option<i64>,
}

/// Store an important decision
pub async fn store_decision(ctx: &OpContext, input: StoreDecisionInput) -> CoreResult<()> {
    if input.key.is_empty() {
        return Err(CoreError::MissingField("key"));
    }
    if input.decision.is_empty() {
        return Err(CoreError::MissingField("decision"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, created_at, updated_at, project_id)
        VALUES ($1, 'decision', $2, $3, $4, $5, 1.0, $6, $6, $7)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            project_id = COALESCE(excluded.project_id, memory_facts.project_id),
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&id)
    .bind(&input.key)
    .bind(&input.decision)
    .bind(&input.category)
    .bind(&input.context)
    .bind(now)
    .bind(input.project_id)
    .execute(db)
    .await?;

    // Store in semantic search
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            use mira_core::semantic::COLLECTION_CONVERSATION;
            use mira_core::semantic_helpers::{store_with_logging, MetadataBuilder};

            let metadata = MetadataBuilder::new("decision")
                .string("key", &input.key)
                .string_opt("category", input.category.as_ref())
                .project_id(input.project_id)
                .build();
            store_with_logging(semantic, COLLECTION_CONVERSATION, &id, &input.decision, metadata)
                .await;
        }
    }

    Ok(())
}

// ============================================================================
// Rejected Approach Operations
// ============================================================================

/// Input for recording a rejected approach
#[derive(Debug, Clone)]
pub struct RecordRejectedApproachInput {
    pub problem_context: String,
    pub approach: String,
    pub rejection_reason: String,
    pub related_files: Option<String>,
    pub related_topics: Option<String>,
    pub project_id: Option<i64>,
}

/// Result of recording a rejected approach
#[derive(Debug, Clone)]
pub struct RecordRejectedApproachOutput {
    pub id: String,
    pub problem_context: String,
    pub approach: String,
}

/// Record a rejected approach
pub async fn record_rejected_approach(
    ctx: &OpContext,
    input: RecordRejectedApproachInput,
) -> CoreResult<RecordRejectedApproachOutput> {
    if input.problem_context.is_empty() {
        return Err(CoreError::MissingField("problem_context"));
    }
    if input.approach.is_empty() {
        return Err(CoreError::MissingField("approach"));
    }
    if input.rejection_reason.is_empty() {
        return Err(CoreError::MissingField("rejection_reason"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let id = format!(
        "rej-{}",
        Uuid::new_v4().to_string().split('-').next().unwrap()
    );

    let related_files = normalize_json_array(&input.related_files);
    let related_topics = normalize_json_array(&input.related_topics);

    sqlx::query(
        r#"
        INSERT INTO rejected_approaches (id, project_id, problem_context, approach, rejection_reason,
                                        related_files, related_topics, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(&id)
    .bind(input.project_id)
    .bind(&input.problem_context)
    .bind(&input.approach)
    .bind(&input.rejection_reason)
    .bind(&related_files)
    .bind(&related_topics)
    .bind(now)
    .execute(db)
    .await?;

    // Store in semantic search
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            use mira_core::semantic::COLLECTION_CONVERSATION;
            use mira_core::semantic_helpers::{store_with_logging, MetadataBuilder};

            let content = format!(
                "Rejected approach for {}: {} - Reason: {}",
                input.problem_context, input.approach, input.rejection_reason
            );
            let metadata = MetadataBuilder::new("rejected_approach")
                .string("id", &id)
                .project_id(input.project_id)
                .build();
            store_with_logging(semantic, COLLECTION_CONVERSATION, &id, &content, metadata).await;
        }
    }

    Ok(RecordRejectedApproachOutput {
        id,
        problem_context: input.problem_context,
        approach: input.approach,
    })
}

// ============================================================================
// Helpers
// ============================================================================

fn normalize_json_array(input: &Option<String>) -> Option<String> {
    input.as_ref().map(|s| {
        if s.trim().starts_with('[') {
            s.clone()
        } else {
            let items: Vec<&str> = s
                .split(',')
                .map(|x| x.trim())
                .filter(|x| !x.is_empty())
                .collect();
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
        }
    })
}
