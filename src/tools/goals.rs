// src/tools/goals.rs
// Goal and progress tracking - thin wrapper delegating to core::ops::mira
//
// Keeps MCP-specific types separate from the shared core.

use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

use crate::core::ops::mira as core_mira;
use crate::core::OpContext;
use crate::core::SemanticSearch;

use super::types::RecordRejectedApproachRequest;

// Parameter structs matching MCP request types
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
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::CreateGoalInput {
        title: req.title,
        description: req.description,
        success_criteria: req.success_criteria,
        priority: req.priority,
        project_id,
    };

    let output = core_mira::create_goal(&ctx, input).await?;

    Ok(serde_json::json!({
        "status": "created",
        "goal_id": output.goal_id,
        "title": output.title,
        "priority": output.priority,
    }))
}

/// List goals with optional filters
pub async fn list_goals(
    db: &SqlitePool,
    req: ListGoalsParams,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::ListGoalsInput {
        status: req.status,
        include_finished: req.include_finished.unwrap_or(false),
        limit: req.limit.unwrap_or(10),
        project_id,
    };

    let goals = core_mira::list_goals(&ctx, input).await?;

    Ok(goals
        .into_iter()
        .map(|g| {
            serde_json::json!({
                "id": g.id,
                "title": g.title,
                "description": g.description,
                "status": g.status,
                "priority": g.priority,
                "progress_percent": g.progress_percent,
                "milestones_completed": g.milestones_completed,
                "milestones_total": g.milestones_total,
                "has_blockers": g.has_blockers,
                "updated_at": g.updated_at,
            })
        })
        .collect())
}

/// Get detailed goal information
pub async fn get_goal(db: &SqlitePool, goal_id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::ListGoalsInput {
        status: None,
        include_finished: true,
        limit: 100,
        project_id: None,
    };

    let goals = core_mira::list_goals(&ctx, input).await?;

    Ok(goals.into_iter().find(|g| g.id == goal_id).map(|g| {
        serde_json::json!({
            "id": g.id,
            "title": g.title,
            "description": g.description,
            "status": g.status,
            "priority": g.priority,
            "progress_percent": g.progress_percent,
            "milestones_completed": g.milestones_completed,
            "milestones_total": g.milestones_total,
            "has_blockers": g.has_blockers,
            "updated_at": g.updated_at,
        })
    }))
}

/// Update a goal
pub async fn update_goal(
    db: &SqlitePool,
    req: UpdateGoalParams,
) -> anyhow::Result<Option<serde_json::Value>> {
    let goal_id = req.goal_id.clone();
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::UpdateGoalInput {
        goal_id: req.goal_id,
        title: req.title,
        description: req.description,
        status: req.status,
        priority: req.priority,
        progress_percent: req.progress_percent,
    };

    let updated = core_mira::update_goal(&ctx, input).await?;

    if !updated {
        return Ok(None);
    }

    get_goal(db, &goal_id).await
}

/// Add a milestone to a goal
pub async fn add_milestone(
    db: &SqlitePool,
    req: AddMilestoneParams,
) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::AddMilestoneInput {
        goal_id: req.goal_id,
        title: req.title,
        description: req.description,
        weight: req.weight,
    };

    let output = core_mira::add_milestone(&ctx, input).await?;

    Ok(serde_json::json!({
        "status": "created",
        "milestone_id": output.milestone_id,
        "goal_id": output.goal_id,
        "title": output.title,
    }))
}

/// Complete a milestone
pub async fn complete_milestone(
    db: &SqlitePool,
    milestone_id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let output = core_mira::complete_milestone(&ctx, milestone_id).await?;

    Ok(output.map(|o| {
        serde_json::json!({
            "status": "completed",
            "milestone_id": o.milestone_id,
            "goal_id": o.goal_id,
            "goal_progress_percent": o.goal_progress_percent,
            "milestones_completed": o.milestones_completed,
            "milestones_total": o.milestones_total,
            "all_milestones_complete": o.all_milestones_complete,
        })
    }))
}

/// Record an approach that was tried and rejected
pub async fn record_rejected_approach(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    req: RecordRejectedApproachRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone())
        .with_semantic(semantic.clone());

    let input = core_mira::RecordRejectedApproachInput {
        problem_context: req.problem_context,
        approach: req.approach,
        rejection_reason: req.rejection_reason,
        related_files: req.related_files,
        related_topics: req.related_topics,
        project_id,
    };

    let output = core_mira::record_rejected_approach(&ctx, input).await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "id": output.id,
        "problem_context": output.problem_context,
        "approach": output.approach,
    }))
}

/// Delete a goal and its milestones
pub async fn delete_goal(db: &SqlitePool, goal_id: &str) -> anyhow::Result<Option<String>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());
    Ok(core_mira::delete_goal(&ctx, goal_id).await?)
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

    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::ListGoalsInput {
        status: None,
        include_finished: false,
        limit: 20,
        project_id,
    };

    let goals = core_mira::list_goals(&ctx, input).await?;

    let total_active = goals.len();
    let blocked_count = goals.iter().filter(|g| g.status == "blocked").count();

    let goals_json: Vec<serde_json::Value> = goals
        .into_iter()
        .map(|g| {
            serde_json::json!({
                "id": g.id,
                "title": g.title,
                "description": g.description,
                "status": g.status,
                "priority": g.priority,
                "progress_percent": g.progress_percent,
                "milestones_completed": g.milestones_completed,
                "milestones_total": g.milestones_total,
                "has_blockers": g.has_blockers,
                "updated_at": g.updated_at,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "active_goals": goals_json,
        "total_active": total_active,
        "blocked_count": blocked_count,
    }))
}
