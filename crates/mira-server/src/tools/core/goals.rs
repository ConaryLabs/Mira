//! crates/mira-server/src/tools/core/goals.rs
//! Goal and milestone tools - split into focused action functions

use crate::db::{
    complete_milestone_sync, create_goal_sync, create_milestone_sync, delete_goal_sync,
    delete_milestone_sync, get_active_goals_sync, get_goal_by_id_sync, get_goals_sync,
    get_milestones_for_goal_sync, update_goal_progress_from_milestones_sync, update_goal_sync,
};
use crate::mcp::requests::{GoalAction, GoalRequest};
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    GoalBulkCreatedData, GoalCreatedData, GoalCreatedEntry, GoalData, GoalGetData, GoalListData,
    GoalOutput, GoalSummary, MilestoneInfo, MilestoneProgressData,
};
use crate::tools::core::ToolContext;
use serde::Deserialize;

/// Goal definition for bulk creation
#[derive(Debug, Deserialize)]
struct BulkGoal {
    title: String,
    description: Option<String>,
    priority: Option<String>,
    status: Option<String>,
}

// ============================================================================
// Action-specific functions
// ============================================================================

/// Get a goal by ID with its milestones
async fn action_get<C: ToolContext>(ctx: &C, goal_id: &str) -> Result<Json<GoalOutput>, String> {
    let id: i64 = goal_id.parse().map_err(|_| "Invalid goal ID")?;

    let goal = ctx
        .pool()
        .run(move |conn| get_goal_by_id_sync(conn, id))
        .await?
        .ok_or_else(|| format!("Goal {} not found", id))?;

    let mut response = format!("Goal [{}]: {}\n", goal.id, goal.title);
    response.push_str(&format!("  Status: {}\n", goal.status));
    response.push_str(&format!("  Priority: {}\n", goal.priority));
    response.push_str(&format!("  Progress: {}%\n", goal.progress_percent));
    if let Some(desc) = &goal.description {
        response.push_str(&format!("  Description: {}\n", desc));
    }
    response.push_str(&format!("  Created: {}\n", goal.created_at));

    // Show milestones
    let milestones = ctx
        .pool()
        .run(move |conn| get_milestones_for_goal_sync(conn, id))
        .await?;

    let mut milestone_items = Vec::new();
    if !milestones.is_empty() {
        response.push_str(&format!("\n  Milestones ({}):\n", milestones.len()));
        for m in &milestones {
            let icon = if m.completed { "v" } else { "o" };
            response.push_str(&format!(
                "    {} [{}] {} (weight: {})\n",
                icon, m.id, m.title, m.weight
            ));
            milestone_items.push(MilestoneInfo {
                id: m.id,
                title: m.title.clone(),
                weight: m.weight,
                completed: m.completed,
            });
        }
    }

    Ok(Json(GoalOutput {
        action: "get".into(),
        message: response,
        data: Some(GoalData::Get(GoalGetData {
            id: goal.id,
            title: goal.title,
            status: goal.status,
            priority: goal.priority,
            progress_percent: goal.progress_percent,
            description: goal.description,
            created_at: goal.created_at,
            milestones: milestone_items,
        })),
    }))
}

/// Create a new goal
async fn action_create<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
    title: String,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    progress_percent: Option<i32>,
) -> Result<Json<GoalOutput>, String> {
    let title_for_result = title.clone();

    let id = ctx
        .pool()
        .run(move |conn| {
            create_goal_sync(
                conn,
                project_id,
                &title,
                description.as_deref(),
                status.as_deref(),
                priority.as_deref(),
                progress_percent.map(|p| p as i64),
            )
        })
        .await?;

    Ok(Json(GoalOutput {
        action: "create".into(),
        message: format!("Created goal '{}' (id: {})", title_for_result, id),
        data: Some(GoalData::Created(GoalCreatedData { goal_id: id })),
    }))
}

/// Bulk create multiple goals
async fn action_bulk_create<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
    goals_json: &str,
) -> Result<Json<GoalOutput>, String> {
    let bulk_goals: Vec<BulkGoal> = serde_json::from_str(goals_json).map_err(|e| {
        format!(
            "Invalid goals JSON: {}. Expected: [{{\"title\": \"...\", \"description?\": \"...\", \"priority?\": \"...\"}}]",
            e
        )
    })?;

    if bulk_goals.is_empty() {
        return Err("goals array cannot be empty".to_string());
    }

    let mut created = Vec::new();
    let mut entries = Vec::new();
    for g in bulk_goals {
        let title = g.title.clone();
        let description = g.description.clone();
        let status = g.status.clone();
        let priority = g.priority.clone();

        let id = ctx
            .pool()
            .run(move |conn| {
                create_goal_sync(
                    conn,
                    project_id,
                    &title,
                    description.as_deref(),
                    status.as_deref(),
                    priority.as_deref(),
                    None,
                )
            })
            .await?;

        created.push(format!("[{}] {}", id, g.title));
        entries.push(GoalCreatedEntry { id, title: g.title });
    }

    Ok(Json(GoalOutput {
        action: "bulk_create".into(),
        message: format!(
            "Created {} goals:\n  {}",
            created.len(),
            created.join("\n  ")
        ),
        data: Some(GoalData::BulkCreated(GoalBulkCreatedData {
            goals: entries,
        })),
    }))
}

/// List goals with optional filters
async fn action_list<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
    include_finished: bool,
    limit: i64,
) -> Result<Json<GoalOutput>, String> {
    let goals = if include_finished {
        ctx.pool()
            .run(move |conn| get_goals_sync(conn, project_id, None))
            .await?
    } else {
        ctx.pool()
            .run(move |conn| get_active_goals_sync(conn, project_id, 100))
            .await?
    };

    let goals: Vec<_> = goals.into_iter().take(limit as usize).collect();

    if goals.is_empty() {
        return Ok(Json(GoalOutput {
            action: "list".into(),
            message: "No goals found.".into(),
            data: Some(GoalData::List(GoalListData {
                goals: vec![],
                total: 0,
            })),
        }));
    }

    // Fetch milestones for all goals in one pass
    let goal_ids: Vec<i64> = goals.iter().map(|g| g.id).collect();
    let milestones_by_goal = {
        let ids = goal_ids.clone();
        ctx.pool()
            .run(move |conn| -> rusqlite::Result<std::collections::HashMap<i64, Vec<MilestoneInfo>>> {
                let mut map = std::collections::HashMap::new();
                for gid in ids {
                    let milestones = get_milestones_for_goal_sync(conn, gid)?;
                    if !milestones.is_empty() {
                        map.insert(
                            gid,
                            milestones
                                .into_iter()
                                .map(|m| MilestoneInfo {
                                    id: m.id,
                                    title: m.title,
                                    weight: m.weight,
                                    completed: m.completed,
                                })
                                .collect(),
                        );
                    }
                }
                Ok(map)
            })
            .await?
    };

    let mut response = format!("{} goals:\n", goals.len());
    let items: Vec<GoalSummary> = goals
        .into_iter()
        .map(|goal| {
            let icon = match goal.status.as_str() {
                "completed" => "v",
                "in_progress" => ">",
                "abandoned" => "x",
                _ => "o",
            };
            let ms = milestones_by_goal
                .get(&goal.id)
                .cloned()
                .unwrap_or_default();
            response.push_str(&format!(
                "  {} {} ({}%) - {} [{}]\n",
                icon, goal.title, goal.progress_percent, goal.priority, goal.id
            ));
            if !ms.is_empty() {
                for m in &ms {
                    let mi = if m.completed { "v" } else { "o" };
                    response.push_str(&format!("    {} {} (w:{})\n", mi, m.title, m.weight));
                }
            }
            GoalSummary {
                id: goal.id,
                title: goal.title,
                status: goal.status,
                priority: goal.priority,
                progress_percent: goal.progress_percent,
                milestones: ms,
            }
        })
        .collect();
    let total = items.len();
    Ok(Json(GoalOutput {
        action: "list".into(),
        message: response,
        data: Some(GoalData::List(GoalListData {
            goals: items,
            total,
        })),
    }))
}

/// Update a goal
async fn action_update<C: ToolContext>(
    ctx: &C,
    goal_id: &str,
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    progress_percent: Option<i32>,
) -> Result<Json<GoalOutput>, String> {
    let id: i64 = goal_id.parse().map_err(|_| "Invalid goal ID")?;

    ctx.pool()
        .run(move |conn| {
            update_goal_sync(
                conn,
                id,
                title.as_deref(),
                description.as_deref(),
                status.as_deref(),
                priority.as_deref(),
                progress_percent.map(|p| p as i64),
            )
        })
        .await?;

    Ok(Json(GoalOutput {
        action: "update".into(),
        message: format!("Updated goal {}", id),
        data: None,
    }))
}

/// Delete a goal
async fn action_delete<C: ToolContext>(ctx: &C, goal_id: &str) -> Result<Json<GoalOutput>, String> {
    let id: i64 = goal_id.parse().map_err(|_| "Invalid goal ID")?;

    ctx.pool()
        .run(move |conn| delete_goal_sync(conn, id))
        .await?;

    Ok(Json(GoalOutput {
        action: "delete".into(),
        message: format!("Deleted goal {}", id),
        data: None,
    }))
}

/// Add a milestone to a goal
async fn action_add_milestone<C: ToolContext>(
    ctx: &C,
    goal_id: &str,
    milestone_title: String,
    weight: Option<i32>,
) -> Result<Json<GoalOutput>, String> {
    let gid: i64 = goal_id.parse().map_err(|_| "Invalid goal ID")?;
    let mtitle_for_result = milestone_title.clone();

    let mid = ctx
        .pool()
        .run(move |conn| create_milestone_sync(conn, gid, &milestone_title, weight))
        .await?;

    Ok(Json(GoalOutput {
        action: "add_milestone".into(),
        message: format!(
            "Added milestone '{}' to goal {} (milestone id: {})",
            mtitle_for_result, gid, mid
        ),
        data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
            milestone_id: mid,
            goal_id: Some(gid),
            progress_percent: None,
        })),
    }))
}

/// Complete a milestone and update goal progress
async fn action_complete_milestone<C: ToolContext>(
    ctx: &C,
    milestone_id: &str,
) -> Result<Json<GoalOutput>, String> {
    let mid: i64 = milestone_id.parse().map_err(|_| "Invalid milestone ID")?;

    let goal_id_result = ctx
        .pool()
        .run(move |conn| complete_milestone_sync(conn, mid))
        .await?;

    if let Some(gid) = goal_id_result {
        let progress = ctx
            .pool()
            .run(move |conn| update_goal_progress_from_milestones_sync(conn, gid))
            .await?;

        Ok(Json(GoalOutput {
            action: "complete_milestone".into(),
            message: format!(
                "Completed milestone {}. Goal progress updated to {}%",
                mid, progress
            ),
            data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
                milestone_id: mid,
                goal_id: Some(gid),
                progress_percent: Some(progress),
            })),
        }))
    } else {
        Ok(Json(GoalOutput {
            action: "complete_milestone".into(),
            message: format!("Completed milestone {}", mid),
            data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
                milestone_id: mid,
                goal_id: None,
                progress_percent: None,
            })),
        }))
    }
}

/// Delete a milestone and update goal progress
async fn action_delete_milestone<C: ToolContext>(
    ctx: &C,
    milestone_id: &str,
) -> Result<Json<GoalOutput>, String> {
    let mid: i64 = milestone_id.parse().map_err(|_| "Invalid milestone ID")?;

    let goal_id_result = ctx
        .pool()
        .run(move |conn| delete_milestone_sync(conn, mid))
        .await?;

    if let Some(gid) = goal_id_result {
        let progress = ctx
            .pool()
            .run(move |conn| update_goal_progress_from_milestones_sync(conn, gid))
            .await?;

        Ok(Json(GoalOutput {
            action: "delete_milestone".into(),
            message: format!(
                "Deleted milestone {}. Goal progress updated to {}%",
                mid, progress
            ),
            data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
                milestone_id: mid,
                goal_id: Some(gid),
                progress_percent: Some(progress),
            })),
        }))
    } else {
        Ok(Json(GoalOutput {
            action: "delete_milestone".into(),
            message: format!("Deleted milestone {}", mid),
            data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
                milestone_id: mid,
                goal_id: None,
                progress_percent: None,
            })),
        }))
    }
}

// ============================================================================
// Main dispatcher
// ============================================================================

/// Unified goal tool with actions: create, bulk_create, list, get, update, progress, delete,
/// add_milestone, complete_milestone, delete_milestone
pub async fn goal<C: ToolContext>(ctx: &C, req: GoalRequest) -> Result<Json<GoalOutput>, String> {
    let project_id = ctx.project_id().await;

    match req.action {
        GoalAction::Get => {
            let id = req.goal_id.ok_or("Goal ID is required for get action")?;
            action_get(ctx, &id).await
        }
        GoalAction::Create => {
            let t = req.title.ok_or("Title is required for create action")?;
            action_create(
                ctx,
                project_id,
                t,
                req.description,
                req.status,
                req.priority,
                req.progress_percent,
            )
            .await
        }
        GoalAction::BulkCreate => {
            let g = req
                .goals
                .ok_or("goals parameter is required for bulk_create action")?;
            action_bulk_create(ctx, project_id, &g).await
        }
        GoalAction::List => {
            action_list(
                ctx,
                project_id,
                req.include_finished.unwrap_or(false),
                req.limit.unwrap_or(10),
            )
            .await
        }
        GoalAction::Update | GoalAction::Progress => {
            let id = req
                .goal_id
                .ok_or("Goal ID is required for update/progress action")?;
            action_update(
                ctx,
                &id,
                req.title,
                req.description,
                req.status,
                req.priority,
                req.progress_percent,
            )
            .await
        }
        GoalAction::Delete => {
            let id = req.goal_id.ok_or("Goal ID is required for delete action")?;
            action_delete(ctx, &id).await
        }
        GoalAction::AddMilestone => {
            let gid = req
                .goal_id
                .ok_or("Goal ID is required for add_milestone action")?;
            let mt = req
                .milestone_title
                .ok_or("milestone_title is required for add_milestone action")?;
            action_add_milestone(ctx, &gid, mt, req.weight).await
        }
        GoalAction::CompleteMilestone => {
            let mid = req
                .milestone_id
                .ok_or("milestone_id is required for complete_milestone action")?;
            action_complete_milestone(ctx, &mid).await
        }
        GoalAction::DeleteMilestone => {
            let mid = req
                .milestone_id
                .ok_or("milestone_id is required for delete_milestone action")?;
            action_delete_milestone(ctx, &mid).await
        }
    }
}
