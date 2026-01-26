//! crates/mira-server/src/tools/core/goals.rs
//! Goal and milestone tools - split into focused action functions

use crate::db::{
    complete_milestone_sync, create_goal_sync, create_milestone_sync, delete_goal_sync,
    delete_milestone_sync, get_active_goals_sync, get_goal_by_id_sync, get_goals_sync,
    get_milestones_for_goal_sync, update_goal_progress_from_milestones_sync, update_goal_sync,
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
async fn action_get<C: ToolContext>(ctx: &C, goal_id: &str) -> Result<String, String> {
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

    if !milestones.is_empty() {
        response.push_str(&format!("\n  Milestones ({}):\n", milestones.len()));
        for m in milestones {
            let icon = if m.completed { "v" } else { "o" };
            response.push_str(&format!(
                "    {} [{}] {} (weight: {})\n",
                icon, m.id, m.title, m.weight
            ));
        }
    }
    Ok(response)
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
) -> Result<String, String> {
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

    Ok(format!("Created goal '{}' (id: {})", title_for_result, id))
}

/// Bulk create multiple goals
async fn action_bulk_create<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
    goals_json: &str,
) -> Result<String, String> {
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
    }

    Ok(format!(
        "Created {} goals:\n  {}",
        created.len(),
        created.join("\n  ")
    ))
}

/// List goals with optional filters
async fn action_list<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
    include_finished: bool,
    limit: i64,
) -> Result<String, String> {
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
        return Ok("No goals found.".to_string());
    }

    let mut response = format!("{} goals:\n", goals.len());
    for goal in goals {
        let icon = match goal.status.as_str() {
            "completed" => "v",
            "in_progress" => ">",
            "abandoned" => "x",
            _ => "o",
        };
        response.push_str(&format!(
            "  {} {} ({}%) - {} [{}]\n",
            icon, goal.title, goal.progress_percent, goal.priority, goal.id
        ));
    }
    Ok(response)
}

/// Update a goal
async fn action_update<C: ToolContext>(
    ctx: &C,
    goal_id: &str,
    title: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    progress_percent: Option<i32>,
) -> Result<String, String> {
    let id: i64 = goal_id.parse().map_err(|_| "Invalid goal ID")?;

    ctx.pool()
        .run(move |conn| {
            update_goal_sync(
                conn,
                id,
                title.as_deref(),
                status.as_deref(),
                priority.as_deref(),
                progress_percent.map(|p| p as i64),
            )
        })
        .await?;

    Ok(format!("Updated goal {}", id))
}

/// Delete a goal
async fn action_delete<C: ToolContext>(ctx: &C, goal_id: &str) -> Result<String, String> {
    let id: i64 = goal_id.parse().map_err(|_| "Invalid goal ID")?;

    ctx.pool()
        .run(move |conn| delete_goal_sync(conn, id))
        .await?;

    Ok(format!("Deleted goal {}", id))
}

/// Add a milestone to a goal
async fn action_add_milestone<C: ToolContext>(
    ctx: &C,
    goal_id: &str,
    milestone_title: String,
    weight: Option<i32>,
) -> Result<String, String> {
    let gid: i64 = goal_id.parse().map_err(|_| "Invalid goal ID")?;
    let mtitle_for_result = milestone_title.clone();

    let mid = ctx
        .pool()
        .run(move |conn| create_milestone_sync(conn, gid, &milestone_title, weight))
        .await?;

    Ok(format!(
        "Added milestone '{}' to goal {} (milestone id: {})",
        mtitle_for_result, gid, mid
    ))
}

/// Complete a milestone and update goal progress
async fn action_complete_milestone<C: ToolContext>(
    ctx: &C,
    milestone_id: &str,
) -> Result<String, String> {
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

        Ok(format!(
            "Completed milestone {}. Goal progress updated to {}%",
            mid, progress
        ))
    } else {
        Ok(format!("Completed milestone {}", mid))
    }
}

/// Delete a milestone and update goal progress
async fn action_delete_milestone<C: ToolContext>(
    ctx: &C,
    milestone_id: &str,
) -> Result<String, String> {
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

        Ok(format!(
            "Deleted milestone {}. Goal progress updated to {}%",
            mid, progress
        ))
    } else {
        Ok(format!("Deleted milestone {}", mid))
    }
}

// ============================================================================
// Main dispatcher
// ============================================================================

/// Unified goal tool with actions: create, bulk_create, list, get, update, progress, delete,
/// add_milestone, complete_milestone, delete_milestone
pub async fn goal<C: ToolContext>(
    ctx: &C,
    action: String,
    goal_id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    progress_percent: Option<i32>,
    include_finished: Option<bool>,
    limit: Option<i64>,
    goals: Option<String>,
    milestone_title: Option<String>,
    milestone_id: Option<String>,
    weight: Option<i32>,
) -> Result<String, String> {
    let project_id = ctx.project_id().await;

    match action.as_str() {
        "get" => {
            let id = goal_id.ok_or("Goal ID is required for get action")?;
            action_get(ctx, &id).await
        }
        "create" => {
            let t = title.ok_or("Title is required for create action")?;
            action_create(ctx, project_id, t, description, status, priority, progress_percent).await
        }
        "bulk_create" => {
            let g = goals.ok_or("goals parameter is required for bulk_create action")?;
            action_bulk_create(ctx, project_id, &g).await
        }
        "list" => {
            action_list(
                ctx,
                project_id,
                include_finished.unwrap_or(false),
                limit.unwrap_or(10),
            )
            .await
        }
        "update" | "progress" => {
            let id = goal_id.ok_or("Goal ID is required for update/progress action")?;
            action_update(ctx, &id, title, status, priority, progress_percent).await
        }
        "delete" => {
            let id = goal_id.ok_or("Goal ID is required for delete action")?;
            action_delete(ctx, &id).await
        }
        "add_milestone" => {
            let gid = goal_id.ok_or("Goal ID is required for add_milestone action")?;
            let mt = milestone_title.ok_or("milestone_title is required for add_milestone action")?;
            action_add_milestone(ctx, &gid, mt, weight).await
        }
        "complete_milestone" => {
            let mid = milestone_id.ok_or("milestone_id is required for complete_milestone action")?;
            action_complete_milestone(ctx, &mid).await
        }
        "delete_milestone" => {
            let mid = milestone_id.ok_or("milestone_id is required for delete_milestone action")?;
            action_delete_milestone(ctx, &mid).await
        }
        _ => Err(format!(
            "Unknown action: {}. Valid actions: create, bulk_create, list, get, update, progress, delete, add_milestone, complete_milestone, delete_milestone",
            action
        )),
    }
}
