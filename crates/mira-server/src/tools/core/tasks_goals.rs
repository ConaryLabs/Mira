//! crates/mira-server/src/tools/core/tasks_goals.rs
//! Unified task and goal tools

use crate::tools::core::ToolContext;

/// Unified task tool with actions: create, list, update, complete, delete
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
) -> Result<String, String> {
    let project_id = ctx.project_id().await;

    match action.as_str() {
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
        _ => Err(format!("Unknown action: {}. Valid actions: create, list, update, complete, delete", action))
    }
}

/// Unified goal tool with actions: create, list, update, progress, delete
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
) -> Result<String, String> {
    let project_id = ctx.project_id().await;

    match action.as_str() {
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
        "list" => {
            let include_finished = include_finished.unwrap_or(false);
            let status_filter = if include_finished { None } else { Some("!finished") };
            let goals = ctx.db().get_goals(project_id, status_filter).map_err(|e| e.to_string())?;

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
        _ => Err(format!("Unknown action: {}. Valid actions: create, list, update, progress, delete", action))
    }
}
