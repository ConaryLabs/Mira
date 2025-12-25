//! Goal and milestone operations

use crate::core::{CoreError, CoreResult, OpContext};
use chrono::Utc;
use uuid::Uuid;

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

/// Result of adding a milestone
#[derive(Debug, Clone)]
pub struct AddMilestoneOutput {
    pub milestone_id: String,
    pub goal_id: String,
    pub title: String,
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
            WHERE ($1 IS NULL OR project_id IS NULL OR project_id = $1)
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
            WHERE ($1 IS NULL OR project_id IS NULL OR project_id = $1)
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

// Helper functions

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
