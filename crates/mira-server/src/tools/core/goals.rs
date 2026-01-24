//! crates/mira-server/src/tools/core/goals.rs
//! Goal and milestone tools

use crate::db::{
    create_goal_sync, delete_goal_sync, get_active_goals_sync, get_goal_by_id_sync,
    get_goals_sync, update_goal_sync,
    create_milestone_sync, get_milestones_for_goal_sync, complete_milestone_sync,
    delete_milestone_sync, update_goal_progress_from_milestones_sync,
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
            let id: i64 = goal_id
                .ok_or("Goal ID is required for get action".to_string())?
                .parse()
                .map_err(|_| "Invalid goal ID".to_string())?;

            let goal = ctx
                .pool()
                .interact(move |conn| {
                    get_goal_by_id_sync(conn, id).map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?
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
                .interact(move |conn| {
                    get_milestones_for_goal_sync(conn, id).map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

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
        "create" => {
            let title = title.ok_or("Title is required for create action".to_string())?;
            let title_clone = title.clone();
            let description_clone = description.clone();
            let status_clone = status.clone();
            let priority_clone = priority.clone();

            let id = ctx
                .pool()
                .interact(move |conn| {
                    create_goal_sync(
                        conn,
                        project_id,
                        &title_clone,
                        description_clone.as_deref(),
                        status_clone.as_deref(),
                        priority_clone.as_deref(),
                        progress_percent.map(|p| p as i64),
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            Ok(format!("Created goal '{}' (id: {})", title, id))
        }
        "bulk_create" => {
            let goals_json = goals.ok_or("goals parameter is required for bulk_create action")?;
            let bulk_goals: Vec<BulkGoal> = serde_json::from_str(&goals_json).map_err(|e| {
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
                    .interact(move |conn| {
                        create_goal_sync(
                            conn,
                            project_id,
                            &title,
                            description.as_deref(),
                            status.as_deref(),
                            priority.as_deref(),
                            None, // progress_percent
                        )
                        .map_err(|e| anyhow::anyhow!("{}", e))
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                created.push(format!("[{}] {}", id, g.title));
            }

            Ok(format!(
                "Created {} goals:\n  {}",
                created.len(),
                created.join("\n  ")
            ))
        }
        "list" => {
            let include_finished = include_finished.unwrap_or(false);
            // Use get_active_goals for non-finished, get_goals(None) for all
            let goals = if include_finished {
                ctx.pool()
                    .interact(move |conn| {
                        get_goals_sync(conn, project_id, None)
                            .map_err(|e| anyhow::anyhow!("{}", e))
                    })
                    .await
                    .map_err(|e| e.to_string())?
            } else {
                // get_active_goals excludes 'completed' and 'abandoned'
                ctx.pool()
                    .interact(move |conn| {
                        get_active_goals_sync(conn, project_id, 100)
                            .map_err(|e| anyhow::anyhow!("{}", e))
                    })
                    .await
                    .map_err(|e| e.to_string())?
            };

            // Apply limit
            let limit = limit.unwrap_or(10) as usize;
            let goals: Vec<_> = goals.into_iter().take(limit).collect();

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
        "update" | "progress" => {
            let id: i64 = goal_id
                .ok_or("Goal ID is required for update/progress action".to_string())?
                .parse()
                .map_err(|_| "Invalid goal ID".to_string())?;

            let title_clone = title.clone();
            let status_clone = status.clone();
            let priority_clone = priority.clone();

            ctx.pool()
                .interact(move |conn| {
                    update_goal_sync(
                        conn,
                        id,
                        title_clone.as_deref(),
                        status_clone.as_deref(),
                        priority_clone.as_deref(),
                        progress_percent.map(|p| p as i64),
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            Ok(format!("Updated goal {}", id))
        }
        "delete" => {
            let id: i64 = goal_id
                .ok_or("Goal ID is required for delete action".to_string())?
                .parse()
                .map_err(|_| "Invalid goal ID".to_string())?;

            ctx.pool()
                .interact(move |conn| {
                    delete_goal_sync(conn, id).map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            Ok(format!("Deleted goal {}", id))
        }
        "add_milestone" => {
            let gid: i64 = goal_id
                .ok_or("Goal ID is required for add_milestone action".to_string())?
                .parse()
                .map_err(|_| "Invalid goal ID".to_string())?;

            let mtitle = milestone_title
                .ok_or("milestone_title is required for add_milestone action".to_string())?;
            let mtitle_clone = mtitle.clone();

            let mid = ctx
                .pool()
                .interact(move |conn| {
                    create_milestone_sync(conn, gid, &mtitle_clone, weight)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            Ok(format!(
                "Added milestone '{}' to goal {} (milestone id: {})",
                mtitle, gid, mid
            ))
        }
        "complete_milestone" => {
            let mid: i64 = milestone_id
                .ok_or("milestone_id is required for complete_milestone action".to_string())?
                .parse()
                .map_err(|_| "Invalid milestone ID".to_string())?;

            let goal_id_result = ctx
                .pool()
                .interact(move |conn| {
                    complete_milestone_sync(conn, mid).map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            // Update goal progress based on milestones
            if let Some(gid) = goal_id_result {
                let progress = ctx
                    .pool()
                    .interact(move |conn| {
                        update_goal_progress_from_milestones_sync(conn, gid)
                            .map_err(|e| anyhow::anyhow!("{}", e))
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                Ok(format!(
                    "Completed milestone {}. Goal progress updated to {}%",
                    mid, progress
                ))
            } else {
                Ok(format!("Completed milestone {}", mid))
            }
        }
        "delete_milestone" => {
            let mid: i64 = milestone_id
                .ok_or("milestone_id is required for delete_milestone action".to_string())?
                .parse()
                .map_err(|_| "Invalid milestone ID".to_string())?;

            let goal_id_result = ctx
                .pool()
                .interact(move |conn| {
                    delete_milestone_sync(conn, mid).map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            // Update goal progress based on remaining milestones
            if let Some(gid) = goal_id_result {
                let progress = ctx
                    .pool()
                    .interact(move |conn| {
                        update_goal_progress_from_milestones_sync(conn, gid)
                            .map_err(|e| anyhow::anyhow!("{}", e))
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                Ok(format!(
                    "Deleted milestone {}. Goal progress updated to {}%",
                    mid, progress
                ))
            } else {
                Ok(format!("Deleted milestone {}", mid))
            }
        }
        _ => Err(format!(
            "Unknown action: {}. Valid actions: create, bulk_create, list, get, update, progress, delete, add_milestone, complete_milestone, delete_milestone",
            action
        )),
    }
}
