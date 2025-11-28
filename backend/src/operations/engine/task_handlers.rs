// backend/src/operations/engine/task_handlers.rs
// Tool handlers for project task management

use crate::project::tasks::{NewProjectTask, ProjectTaskService, TaskPriority};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::info;

/// Handle the manage_project_task tool
pub async fn handle_manage_project_task(
    task_service: &Arc<ProjectTaskService>,
    args: &Value,
    project_id: Option<&str>,
    session_id: &str,
) -> Result<Value> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required 'action' parameter"))?;

    info!(action = action, "Handling manage_project_task");

    match action {
        "create" => handle_create(task_service, args, project_id, session_id).await,
        "update" => handle_update(task_service, args).await,
        "complete" => handle_complete(task_service, args).await,
        "list" => handle_list(task_service, project_id).await,
        _ => Err(anyhow!("Unknown action: {}", action)),
    }
}

/// Create a new task
async fn handle_create(
    task_service: &Arc<ProjectTaskService>,
    args: &Value,
    project_id: Option<&str>,
    session_id: &str,
) -> Result<Value> {
    let project_id = project_id.ok_or_else(|| anyhow!("No project context - cannot create task"))?;

    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required 'title' parameter for create"))?;

    let description = args.get("description").and_then(|v| v.as_str());

    let priority = args
        .get("priority")
        .and_then(|v| v.as_str())
        .map(TaskPriority::from_str)
        .unwrap_or(TaskPriority::Medium);

    let tags: Vec<String> = args
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let input = NewProjectTask {
        project_id: project_id.to_string(),
        title: title.to_string(),
        description: description.map(|s| s.to_string()),
        priority,
        tags,
        parent_task_id: None,
        user_id: None,
    };

    let task = task_service.create_task(input).await?;

    // Start a session for this task
    task_service.start_task(task.id, session_id, None).await?;

    info!(task_id = task.id, title = title, "Created and started task");

    Ok(json!({
        "success": true,
        "task_id": task.id,
        "title": task.title,
        "status": "in_progress",
        "message": format!("Created task #{}: {}", task.id, task.title)
    }))
}

/// Update task progress
async fn handle_update(task_service: &Arc<ProjectTaskService>, args: &Value) -> Result<Value> {
    let task_id = args
        .get("task_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("Missing required 'task_id' parameter for update"))?;

    let progress_notes = args
        .get("progress_notes")
        .or_else(|| args.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("Progress update");

    task_service.update_progress(task_id, progress_notes).await?;

    info!(task_id = task_id, "Updated task progress");

    Ok(json!({
        "success": true,
        "task_id": task_id,
        "message": format!("Updated task #{} with progress notes", task_id)
    }))
}

/// Complete a task
async fn handle_complete(task_service: &Arc<ProjectTaskService>, args: &Value) -> Result<Value> {
    let task_id = args
        .get("task_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("Missing required 'task_id' parameter for complete"))?;

    let summary = args
        .get("progress_notes")
        .or_else(|| args.get("description"))
        .and_then(|v| v.as_str());

    task_service.complete_task(task_id, summary).await?;

    info!(task_id = task_id, "Completed task");

    Ok(json!({
        "success": true,
        "task_id": task_id,
        "status": "completed",
        "message": format!("Task #{} marked as completed", task_id)
    }))
}

/// List incomplete tasks
async fn handle_list(
    task_service: &Arc<ProjectTaskService>,
    project_id: Option<&str>,
) -> Result<Value> {
    let project_id =
        project_id.ok_or_else(|| anyhow!("No project context - cannot list tasks"))?;

    let tasks = task_service.get_incomplete_tasks(project_id).await?;

    let task_list: Vec<Value> = tasks
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "title": t.title,
                "status": t.status.as_str(),
                "priority": t.priority,
                "description": t.description,
                "created_at": t.created_at,
                "started_at": t.started_at,
            })
        })
        .collect();

    Ok(json!({
        "success": true,
        "count": task_list.len(),
        "tasks": task_list,
        "message": if task_list.is_empty() {
            "No incomplete tasks".to_string()
        } else {
            format!("{} incomplete task(s)", task_list.len())
        }
    }))
}
