//! crates/mira-server/src/tools/core/tasks_goals.rs
//! Unified task and goal tools

use crate::tools::core::ToolContext;
use serde::Deserialize;

/// Task definition for bulk creation
#[derive(Debug, Deserialize)]
struct BulkTask {
    title: String,
    description: Option<String>,
    priority: Option<String>,
    status: Option<String>,
}

/// Goal definition for bulk creation
#[derive(Debug, Deserialize)]
struct BulkGoal {
    title: String,
    description: Option<String>,
    priority: Option<String>,
    status: Option<String>,
}

/// Unified task tool with actions: create, bulk_create, list, get, update, complete, delete
pub async fn task<C: ToolContext>(
    ctx: &C,
    action: String,
    task_id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    include_completed: Option<bool>,
    limit: Option<i64>,
    tasks: Option<String>,
) -> Result<String, String> {
    let project_id = ctx.project_id().await;

    match action.as_str() {
        "get" => {
            let id: i64 = task_id
                .ok_or("Task ID is required for get action".to_string())?
                .parse()
                .map_err(|_| "Invalid task ID".to_string())?;

            let task = ctx.db().get_task_by_id(id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Task {} not found", id))?;

            let mut response = format!("Task [{}]: {}\n", task.id, task.title);
            response.push_str(&format!("  Status: {}\n", task.status));
            response.push_str(&format!("  Priority: {}\n", task.priority));
            if let Some(desc) = &task.description {
                response.push_str(&format!("  Description: {}\n", desc));
            }
            if let Some(goal_id) = task.goal_id {
                response.push_str(&format!("  Goal: {}\n", goal_id));
            }
            response.push_str(&format!("  Created: {}\n", task.created_at));
            Ok(response)
        }
        "create" => {
            let title = title.ok_or("Title is required for create action".to_string())?;
            let id = ctx.db().create_task(
                project_id,
                None, // goal_id
                &title,
                description.as_deref(),
                status.as_deref(),
                priority.as_deref(),
            ).map_err(|e| e.to_string())?;
            Ok(format!("Created task '{}' (id: {})", title, id))
        }
        "bulk_create" => {
            let tasks_json = tasks.ok_or("tasks parameter is required for bulk_create action")?;
            let bulk_tasks: Vec<BulkTask> = serde_json::from_str(&tasks_json)
                .map_err(|e| format!("Invalid tasks JSON: {}. Expected: [{{\"title\": \"...\", \"description?\": \"...\", \"priority?\": \"...\"}}]", e))?;

            if bulk_tasks.is_empty() {
                return Err("tasks array cannot be empty".to_string());
            }

            let mut created = Vec::new();
            for t in bulk_tasks {
                let id = ctx.db().create_task(
                    project_id,
                    None, // goal_id
                    &t.title,
                    t.description.as_deref(),
                    t.status.as_deref(),
                    t.priority.as_deref(),
                ).map_err(|e| e.to_string())?;
                created.push(format!("[{}] {}", id, t.title));
            }

            Ok(format!("Created {} tasks:\n  {}", created.len(), created.join("\n  ")))
        }
        "list" => {
            let include_completed = include_completed.unwrap_or(false);
            let status_filter = if include_completed { None } else { Some("!completed") };
            let tasks = ctx.db().get_tasks(project_id, status_filter).map_err(|e| e.to_string())?;

            // Apply limit
            let limit = limit.unwrap_or(20) as usize;
            let tasks: Vec<_> = tasks.into_iter().take(limit).collect();

            if tasks.is_empty() {
                return Ok("No tasks found.".to_string());
            }

            let mut response = format!("{} tasks:\n", tasks.len());
            for task in tasks {
                let icon = match task.status.as_str() {
                    "completed" => "v",
                    "in_progress" => ">",
                    "blocked" => "x",
                    _ => "o",
                };
                response.push_str(&format!(
                    "  {} [{}] {} ({})\n",
                    icon, task.id, task.title, task.priority
                ));
            }
            Ok(response)
        }
        "update" | "complete" => {
            let id: i64 = task_id
                .ok_or("Task ID is required for update/complete action".to_string())?
                .parse()
                .map_err(|_| "Invalid task ID".to_string())?;

            let new_status = if action == "complete" {
                Some("completed")
            } else {
                status.as_deref()
            };

            ctx.db().update_task(
                id,
                title.as_deref(),
                new_status,
                priority.as_deref(),
            ).map_err(|e| e.to_string())?;

            Ok(format!("Updated task {}", id))
        }
        "delete" => {
            let id: i64 = task_id
                .ok_or("Task ID is required for delete action".to_string())?
                .parse()
                .map_err(|_| "Invalid task ID".to_string())?;

            ctx.db().delete_task(id).map_err(|e| e.to_string())?;

            Ok(format!("Deleted task {}", id))
        }
        _ => Err(format!("Unknown action: {}. Valid actions: create, bulk_create, list, get, update, complete, delete", action))
    }
}

/// Unified goal tool with actions: create, bulk_create, list, get, update, progress, delete
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
) -> Result<String, String> {
    let project_id = ctx.project_id().await;

    match action.as_str() {
        "get" => {
            let id: i64 = goal_id
                .ok_or("Goal ID is required for get action".to_string())?
                .parse()
                .map_err(|_| "Invalid goal ID".to_string())?;

            let goal = ctx.db().get_goal_by_id(id)
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

            // Also show related tasks
            let tasks = ctx.db().get_tasks(project_id, None)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|t| t.goal_id == Some(id))
                .collect::<Vec<_>>();

            if !tasks.is_empty() {
                response.push_str(&format!("\n  Related tasks ({}):\n", tasks.len()));
                for task in tasks {
                    let icon = match task.status.as_str() {
                        "completed" => "v",
                        "in_progress" => ">",
                        "blocked" => "x",
                        _ => "o",
                    };
                    response.push_str(&format!("    {} [{}] {}\n", icon, task.id, task.title));
                }
            }
            Ok(response)
        }
        "create" => {
            let title = title.ok_or("Title is required for create action".to_string())?;
            let id = ctx.db().create_goal(
                project_id,
                &title,
                description.as_deref(),
                status.as_deref(),
                priority.as_deref(),
                progress_percent.map(|p| p as i64),
            ).map_err(|e| e.to_string())?;
            Ok(format!("Created goal '{}' (id: {})", title, id))
        }
        "bulk_create" => {
            let goals_json = goals.ok_or("goals parameter is required for bulk_create action")?;
            let bulk_goals: Vec<BulkGoal> = serde_json::from_str(&goals_json)
                .map_err(|e| format!("Invalid goals JSON: {}. Expected: [{{\"title\": \"...\", \"description?\": \"...\", \"priority?\": \"...\"}}]", e))?;

            if bulk_goals.is_empty() {
                return Err("goals array cannot be empty".to_string());
            }

            let mut created = Vec::new();
            for g in bulk_goals {
                let id = ctx.db().create_goal(
                    project_id,
                    &g.title,
                    g.description.as_deref(),
                    g.status.as_deref(),
                    g.priority.as_deref(),
                    None, // progress_percent
                ).map_err(|e| e.to_string())?;
                created.push(format!("[{}] {}", id, g.title));
            }

            Ok(format!("Created {} goals:\n  {}", created.len(), created.join("\n  ")))
        }
        "list" => {
            let include_finished = include_finished.unwrap_or(false);
            // Use get_active_goals for non-finished, get_goals(None) for all
            let goals = if include_finished {
                ctx.db().get_goals(project_id, None).map_err(|e| e.to_string())?
            } else {
                // get_active_goals excludes 'completed' and 'abandoned'
                ctx.db().get_active_goals(project_id, 100).map_err(|e| e.to_string())?
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

            ctx.db().update_goal(
                id,
                title.as_deref(),
                status.as_deref(),
                priority.as_deref(),
                progress_percent.map(|p| p as i64),
            ).map_err(|e| e.to_string())?;

            Ok(format!("Updated goal {}", id))
        }
        "delete" => {
            let id: i64 = goal_id
                .ok_or("Goal ID is required for delete action".to_string())?
                .parse()
                .map_err(|_| "Invalid goal ID".to_string())?;

            ctx.db().delete_goal(id).map_err(|e| e.to_string())?;

            Ok(format!("Deleted goal {}", id))
        }
        _ => Err(format!("Unknown action: {}. Valid actions: create, bulk_create, list, get, update, progress, delete", action))
    }
}
