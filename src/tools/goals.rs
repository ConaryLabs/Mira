// src/tools/goals.rs
// Goal and progress tracking - Maintain big picture across sessions

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use super::types::RecordRejectedApproachRequest;

// === Parameter structs for consolidated goal tool ===

pub struct CreateGoalParams {
    pub title: String,
    pub description: Option<String>,
    pub success_criteria: Option<String>,
    pub priority: Option<String>,
}

pub struct ListGoalsParams {
    pub status: Option<String>,
    pub include_finished: Option<bool>,
    pub limit: Option<i64>,
}

pub struct UpdateGoalParams {
    pub goal_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub progress_percent: Option<i32>,
}

pub struct AddMilestoneParams {
    pub goal_id: String,
    pub title: String,
    pub description: Option<String>,
    pub weight: Option<i32>,
}

/// Create a new high-level goal
pub async fn create_goal(
    db: &SqlitePool,
    req: CreateGoalParams,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = format!("goal-{}", Uuid::new_v4().to_string().split('-').next().unwrap());
    let priority = req.priority.as_deref().unwrap_or("medium");

    sqlx::query(r#"
        INSERT INTO goals (id, title, description, success_criteria, status, priority,
                          project_id, created_at, updated_at)
        VALUES ($1, $2, $3, $4, 'planning', $5, $6, $7, $7)
    "#)
    .bind(&id)
    .bind(&req.title)
    .bind(&req.description)
    .bind(&req.success_criteria)
    .bind(priority)
    .bind(project_id)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "created",
        "goal_id": id,
        "title": req.title,
        "priority": priority,
    }))
}

/// List goals with optional filters
pub async fn list_goals(
    db: &SqlitePool,
    req: ListGoalsParams,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(10);
    let include_finished = req.include_finished.unwrap_or(false);

    let results = if include_finished {
        sqlx::query_as::<_, (String, String, Option<String>, String, String, i32, String, Option<String>, String)>(r#"
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
        "#)
        .bind(project_id)
        .bind(&req.status)
        .bind(limit)
        .fetch_all(db)
        .await?
    } else {
        sqlx::query_as::<_, (String, String, Option<String>, String, String, i32, String, Option<String>, String)>(r#"
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
        "#)
        .bind(project_id)
        .bind(&req.status)
        .bind(limit)
        .fetch_all(db)
        .await?
    };

    let mut goals = Vec::new();
    for (id, title, description, status, priority, progress, progress_mode, blockers, updated) in results {
        let actual_progress = if progress_mode == "auto" {
            calculate_goal_progress(db, &id).await.unwrap_or(progress)
        } else {
            progress
        };
        let (total, completed) = get_milestone_counts(db, &id).await.unwrap_or((0, 0));

        goals.push(serde_json::json!({
            "id": id,
            "title": title,
            "description": description,
            "status": status,
            "priority": priority,
            "progress_percent": actual_progress,
            "milestones_completed": completed,
            "milestones_total": total,
            "has_blockers": blockers.map(|b| !b.is_empty()).unwrap_or(false),
            "updated_at": updated,
        }));
    }

    Ok(goals)
}

/// Get detailed goal information
pub async fn get_goal(
    db: &SqlitePool,
    goal_id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let goal = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, String, String, i32, String, Option<String>, Option<String>, Option<String>, String, String)>(r#"
        SELECT id, title, description, success_criteria, status, priority,
               progress_percent, progress_mode, blockers, notes, tags,
               datetime(created_at, 'unixepoch', 'localtime') as created,
               datetime(updated_at, 'unixepoch', 'localtime') as updated
        FROM goals WHERE id = $1
    "#)
    .bind(goal_id)
    .fetch_optional(db)
    .await?;

    let (id, title, description, success_criteria, status, priority, progress,
         progress_mode, blockers, notes, tags, created, updated) = match goal {
        Some(g) => g,
        None => return Ok(None),
    };

    let actual_progress = if progress_mode == "auto" {
        calculate_goal_progress(db, &id).await.unwrap_or(progress)
    } else {
        progress
    };

    let milestones = sqlx::query_as::<_, (String, String, Option<String>, String, i32, i32, Option<String>)>(r#"
        SELECT id, title, description, status, weight, order_index,
               datetime(completed_at, 'unixepoch', 'localtime') as completed
        FROM milestones
        WHERE goal_id = $1
        ORDER BY order_index, created_at
    "#)
    .bind(goal_id)
    .fetch_all(db)
    .await?;

    let milestones_json: Vec<serde_json::Value> = milestones.into_iter().map(|(id, title, desc, status, weight, order, completed)| {
        serde_json::json!({
            "id": id,
            "title": title,
            "description": desc,
            "status": status,
            "weight": weight,
            "order_index": order,
            "completed_at": completed,
        })
    }).collect();

    Ok(Some(serde_json::json!({
        "id": id,
        "title": title,
        "description": description,
        "success_criteria": success_criteria,
        "status": status,
        "priority": priority,
        "progress_percent": actual_progress,
        "progress_mode": progress_mode,
        "blockers": blockers,
        "notes": notes,
        "tags": tags,
        "created_at": created,
        "updated_at": updated,
        "milestones": milestones_json,
    })))
}

/// Update a goal
pub async fn update_goal(
    db: &SqlitePool,
    req: UpdateGoalParams,
) -> anyhow::Result<Option<serde_json::Value>> {
    let now = Utc::now().timestamp();

    let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM goals WHERE id = $1")
        .bind(&req.goal_id)
        .fetch_one(db)
        .await? > 0;

    if !exists {
        return Ok(None);
    }

    if let Some(title) = &req.title {
        sqlx::query("UPDATE goals SET title = $2, updated_at = $3 WHERE id = $1")
            .bind(&req.goal_id)
            .bind(title)
            .bind(now)
            .execute(db)
            .await?;
    }

    if let Some(description) = &req.description {
        sqlx::query("UPDATE goals SET description = $2, updated_at = $3 WHERE id = $1")
            .bind(&req.goal_id)
            .bind(description)
            .bind(now)
            .execute(db)
            .await?;
    }

    if let Some(status) = &req.status {
        let completed_at = if status == "completed" { Some(now) } else { None };
        let started_at = if status == "in_progress" { Some(now) } else { None };

        sqlx::query(r#"
            UPDATE goals
            SET status = $2,
                completed_at = COALESCE($3, completed_at),
                started_at = COALESCE($4, started_at),
                updated_at = $5
            WHERE id = $1
        "#)
            .bind(&req.goal_id)
            .bind(status)
            .bind(completed_at)
            .bind(started_at)
            .bind(now)
            .execute(db)
            .await?;
    }

    if let Some(priority) = &req.priority {
        sqlx::query("UPDATE goals SET priority = $2, updated_at = $3 WHERE id = $1")
            .bind(&req.goal_id)
            .bind(priority)
            .bind(now)
            .execute(db)
            .await?;
    }

    if let Some(progress) = req.progress_percent {
        sqlx::query("UPDATE goals SET progress_percent = $2, progress_mode = 'manual', updated_at = $3 WHERE id = $1")
            .bind(&req.goal_id)
            .bind(progress)
            .bind(now)
            .execute(db)
            .await?;
    }

    get_goal(db, &req.goal_id).await
}

/// Add a milestone to a goal
pub async fn add_milestone(
    db: &SqlitePool,
    req: AddMilestoneParams,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = format!("ms-{}", Uuid::new_v4().to_string().split('-').next().unwrap());
    let weight = req.weight.unwrap_or(1);

    let max_order: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(order_index), -1) FROM milestones WHERE goal_id = $1"
    )
    .bind(&req.goal_id)
    .fetch_one(db)
    .await?;

    sqlx::query(r#"
        INSERT INTO milestones (id, goal_id, title, description, weight, order_index, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
    "#)
    .bind(&id)
    .bind(&req.goal_id)
    .bind(&req.title)
    .bind(&req.description)
    .bind(weight)
    .bind(max_order + 1)
    .bind(now)
    .execute(db)
    .await?;

    sqlx::query("UPDATE goals SET updated_at = $2 WHERE id = $1")
        .bind(&req.goal_id)
        .bind(now)
        .execute(db)
        .await?;

    Ok(serde_json::json!({
        "status": "created",
        "milestone_id": id,
        "goal_id": req.goal_id,
        "title": req.title,
    }))
}

/// Complete a milestone
pub async fn complete_milestone(
    db: &SqlitePool,
    milestone_id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let now = Utc::now().timestamp();

    let milestone = sqlx::query_as::<_, (String, String)>(
        "SELECT id, goal_id FROM milestones WHERE id = $1"
    )
    .bind(milestone_id)
    .fetch_optional(db)
    .await?;

    let (_, goal_id) = match milestone {
        Some(m) => m,
        None => return Ok(None),
    };

    sqlx::query(r#"
        UPDATE milestones
        SET status = 'completed', completed_at = $2, updated_at = $2
        WHERE id = $1
    "#)
    .bind(milestone_id)
    .bind(now)
    .execute(db)
    .await?;

    let new_progress = calculate_goal_progress(db, &goal_id).await?;

    sqlx::query("UPDATE goals SET progress_percent = $2, updated_at = $3 WHERE id = $1 AND progress_mode = 'auto'")
        .bind(&goal_id)
        .bind(new_progress)
        .bind(now)
        .execute(db)
        .await?;

    let (total, completed) = get_milestone_counts(db, &goal_id).await?;

    Ok(Some(serde_json::json!({
        "status": "completed",
        "milestone_id": milestone_id,
        "goal_id": goal_id,
        "goal_progress_percent": new_progress,
        "milestones_completed": completed,
        "milestones_total": total,
        "all_milestones_complete": completed == total,
    })))
}

/// Record an approach that was tried and rejected
pub async fn record_rejected_approach(
    db: &SqlitePool,
    req: RecordRejectedApproachRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = format!("rej-{}", Uuid::new_v4().to_string().split('-').next().unwrap());

    let related_files = normalize_json_array(&req.related_files);
    let related_topics = normalize_json_array(&req.related_topics);

    sqlx::query(r#"
        INSERT INTO rejected_approaches (id, project_id, problem_context, approach, rejection_reason,
                                        related_files, related_topics, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    "#)
    .bind(&id)
    .bind(project_id)
    .bind(&req.problem_context)
    .bind(&req.approach)
    .bind(&req.rejection_reason)
    .bind(&related_files)
    .bind(&related_topics)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "id": id,
        "problem_context": req.problem_context,
        "approach": req.approach,
    }))
}

/// Get progress summary for goals
pub async fn get_goal_progress(
    db: &SqlitePool,
    goal_id: Option<String>,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    if let Some(goal_id) = goal_id {
        let goal = get_goal(db, &goal_id).await?;
        return Ok(goal.unwrap_or(serde_json::json!({"error": "Goal not found"})));
    }

    let goals = list_goals(db, ListGoalsParams {
        status: None,
        include_finished: Some(false),
        limit: Some(20),
    }, project_id).await?;

    let total_active = goals.len();
    let blocked_count = goals.iter()
        .filter(|g| g.get("status").and_then(|s| s.as_str()) == Some("blocked"))
        .count();

    Ok(serde_json::json!({
        "active_goals": goals,
        "total_active": total_active,
        "blocked_count": blocked_count,
    }))
}

// === Helper Functions ===

async fn calculate_goal_progress(db: &SqlitePool, goal_id: &str) -> anyhow::Result<i32> {
    let milestones = sqlx::query_as::<_, (i32, String)>(
        "SELECT weight, status FROM milestones WHERE goal_id = $1"
    )
    .bind(goal_id)
    .fetch_all(db)
    .await?;

    if milestones.is_empty() {
        return Ok(0);
    }

    let total_weight: i32 = milestones.iter().map(|(w, _)| *w).sum();
    let completed_weight: i32 = milestones.iter()
        .filter(|(_, s)| s == "completed")
        .map(|(w, _)| *w)
        .sum();

    if total_weight > 0 {
        Ok(((completed_weight as f64 / total_weight as f64) * 100.0).round() as i32)
    } else {
        Ok(0)
    }
}

async fn get_milestone_counts(db: &SqlitePool, goal_id: &str) -> anyhow::Result<(i64, i64)> {
    let counts = sqlx::query_as::<_, (i64, i64)>(r#"
        SELECT
            COUNT(*) as total,
            SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed
        FROM milestones WHERE goal_id = $1
    "#)
    .bind(goal_id)
    .fetch_one(db)
    .await?;

    Ok(counts)
}

fn normalize_json_array(input: &Option<String>) -> Option<String> {
    input.as_ref().map(|s| {
        if s.trim().starts_with('[') {
            s.clone()
        } else {
            let items: Vec<&str> = s.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).collect();
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
        }
    })
}
