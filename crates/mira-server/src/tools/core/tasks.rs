// crates/mira-server/src/tools/core/tasks.rs
// Fallback "tasks" tool for clients without native MCP task support.
//
// This handler takes &MiraServer directly (not &impl ToolContext) because
// it needs access to the OperationProcessor which is MCP-specific.

use crate::error::MiraError;
use crate::mcp::CachedTaskResult;
use crate::mcp::MiraServer;
use crate::mcp::requests::SessionAction;
use crate::mcp::responses::{
    Json, TaskSummary, TasksData, TasksListData, TasksOutput, TasksStatusData,
};
use crate::utils::truncate;
use rmcp::task_manager::ToolCallTaskResult;
use std::time::{Duration, Instant};

/// How long completed task results are retained in the cache.
const CACHE_RETENTION: Duration = Duration::from_secs(5 * 60);

pub async fn handle_tasks(
    server: &MiraServer,
    action: SessionAction,
    task_id: Option<String>,
) -> Result<Json<TasksOutput>, MiraError> {
    match action {
        SessionAction::TasksList => handle_list(server).await,
        SessionAction::TasksGet => {
            let task_id = task_id.ok_or_else(|| {
                MiraError::InvalidInput(
                    "task_id is required for session(action=tasks_get)".to_string(),
                )
            })?;
            handle_get(server, &task_id).await
        }
        SessionAction::TasksCancel => {
            let task_id = task_id.ok_or_else(|| {
                MiraError::InvalidInput(
                    "task_id is required for session(action=tasks_cancel)".to_string(),
                )
            })?;
            handle_cancel(server, &task_id).await
        }
        _ => Err(MiraError::InvalidInput(
            "Invalid action for tasks handler".to_string(),
        )),
    }
}

/// Status string from a task_manager::TaskResult
fn result_status(
    result: &Result<Box<dyn rmcp::task_manager::OperationResultTransport>, rmcp::RmcpError>,
) -> &'static str {
    match result {
        Ok(_) => "completed",
        Err(e) if e.to_string().contains("cancelled") => "cancelled",
        Err(_) => "failed",
    }
}

async fn handle_list(server: &MiraServer) -> Result<Json<TasksOutput>, MiraError> {
    let mut proc = server.processor.lock().await;
    proc.check_timeouts();

    // Drain channel and collect completed results (this empties the internal buffer)
    let freshly_completed = proc.collect_completed_results();

    let running_ids = proc.list_running();
    drop(proc); // Release processor lock before acquiring cache lock

    let mut summaries: Vec<TaskSummary> = Vec::new();

    // Re-acquire processor for descriptors (cheap lock, no contention with cache)
    {
        let proc = server.processor.lock().await;
        for id in &running_ids {
            if let Some(desc) = proc.task_descriptor(id) {
                summaries.push(TaskSummary {
                    task_id: id.clone(),
                    tool_name: desc.name.clone(),
                    status: "working".to_string(),
                });
            }
        }
    }

    // Cache freshly completed results and build summaries
    let mut cache = server.completed_cache.lock().await;
    let now = Instant::now();

    // Evict entries older than retention window
    cache.retain(|entry| now.duration_since(entry.completed_at) < CACHE_RETENTION);

    for result in &freshly_completed {
        let status = result_status(&result.result).to_string();
        let result_text = extract_result_text(result);

        let cached = CachedTaskResult {
            task_id: result.descriptor.operation_id.clone(),
            tool_name: result.descriptor.name.clone(),
            status: status.clone(),
            result_text,
            completed_at: now,
        };

        // Only add if not already cached (avoid duplicates on repeated list calls)
        if !cache.iter().any(|c| c.task_id == cached.task_id) {
            cache.push(cached);
        }
    }

    // Include all cached entries in the summary (skip any that are still running)
    for entry in cache.iter() {
        if !summaries.iter().any(|s| s.task_id == entry.task_id) {
            summaries.push(TaskSummary {
                task_id: entry.task_id.clone(),
                tool_name: entry.tool_name.clone(),
                status: entry.status.clone(),
            });
        }
    }

    let total = summaries.len();
    let message = if total == 0 {
        "No tasks".to_string()
    } else {
        let running = summaries.iter().filter(|s| s.status == "working").count();
        let completed_count = total - running;
        format!(
            "{} task(s): {} running, {} completed/failed",
            total, running, completed_count
        )
    };

    Ok(Json(TasksOutput {
        action: "list".to_string(),
        message,
        data: Some(TasksData::List(TasksListData {
            tasks: summaries,
            total,
        })),
    }))
}

async fn handle_get(server: &MiraServer, task_id: &str) -> Result<Json<TasksOutput>, MiraError> {
    let mut proc = server.processor.lock().await;

    // Drain channel first — moves finished tasks from running to completed
    let freshly_completed = proc.collect_completed_results();

    // Check freshly completed results first (task may have just finished)
    let position = freshly_completed
        .iter()
        .position(|r| r.descriptor.operation_id == task_id);

    if position.is_none() && proc.list_running().contains(&task_id.to_string()) {
        // Still running
        if let Some(desc) = proc.task_descriptor(task_id) {
            return Ok(Json(TasksOutput {
                action: "get".to_string(),
                message: format!("Task {} is still running ({})", task_id, desc.name),
                data: Some(TasksData::Status(TasksStatusData {
                    task_id: task_id.to_string(),
                    status: "working".to_string(),
                    result_text: None,
                    result_structured: None,
                })),
            }));
        }
    }
    drop(proc); // Release processor lock

    // Cache any freshly completed results we got
    let mut cache = server.completed_cache.lock().await;
    let now = Instant::now();
    cache.retain(|entry| now.duration_since(entry.completed_at) < CACHE_RETENTION);

    for result in &freshly_completed {
        let cached = CachedTaskResult {
            task_id: result.descriptor.operation_id.clone(),
            tool_name: result.descriptor.name.clone(),
            status: result_status(&result.result).to_string(),
            result_text: extract_result_text(result),
            completed_at: now,
        };
        if !cache.iter().any(|c| c.task_id == cached.task_id) {
            cache.push(cached);
        }
    }

    // Try to find the requested task in freshly completed results
    if let Some(idx) = position {
        let Some(task_result) = freshly_completed.into_iter().nth(idx) else {
            return Err(MiraError::InvalidInput(format!(
                "Task '{}' not found. Use session(action=\"tasks_list\") to see available tasks.",
                task_id
            )));
        };

        let (status, result_text, result_structured) = extract_full_result(&task_result);

        let message = match &result_text {
            Some(t) => format!("Task {}: {} — {}", task_id, status, truncate(t, 100)),
            None => format!("Task {}: {}", task_id, status),
        };

        return Ok(Json(TasksOutput {
            action: "get".to_string(),
            message,
            data: Some(TasksData::Status(TasksStatusData {
                task_id: task_id.to_string(),
                status,
                result_text,
                result_structured,
            })),
        }));
    }

    // Fallback: check the cache for previously completed results
    if let Some(entry) = cache.iter().find(|c| c.task_id == task_id) {
        let message = match &entry.result_text {
            Some(t) => format!("Task {}: {} — {}", task_id, entry.status, truncate(t, 100)),
            None => format!("Task {}: {}", task_id, entry.status),
        };

        return Ok(Json(TasksOutput {
            action: "get".to_string(),
            message,
            data: Some(TasksData::Status(TasksStatusData {
                task_id: task_id.to_string(),
                status: entry.status.clone(),
                result_text: entry.result_text.clone(),
                result_structured: None, // Structured content not retained in cache
            })),
        }));
    }

    Err(MiraError::InvalidInput(format!(
        "Task '{}' not found. Use session(action=\"tasks_list\") to see available tasks.",
        task_id
    )))
}

/// Extract just the text portion of a completed result (for caching).
fn extract_result_text(result: &rmcp::task_manager::TaskResult) -> Option<String> {
    match &result.result {
        Ok(boxed) => {
            if let Some(tcr) = boxed.as_any().downcast_ref::<ToolCallTaskResult>() {
                match &tcr.result {
                    Ok(call_result) => call_result
                        .content
                        .first()
                        .and_then(|c| c.as_text())
                        .map(|t| t.text.to_string()),
                    Err(e) => Some(e.message.to_string()),
                }
            } else {
                Some("(result type unknown)".to_string())
            }
        }
        Err(e) => Some(e.to_string()),
    }
}

/// Extract status, text, and structured content from a completed result.
fn extract_full_result(
    result: &rmcp::task_manager::TaskResult,
) -> (String, Option<String>, Option<serde_json::Value>) {
    match &result.result {
        Ok(boxed) => {
            if let Some(tcr) = boxed.as_any().downcast_ref::<ToolCallTaskResult>() {
                match &tcr.result {
                    Ok(call_result) => {
                        let text = call_result
                            .content
                            .first()
                            .and_then(|c| c.as_text())
                            .map(|t| t.text.to_string());
                        let structured = call_result.structured_content.clone();
                        ("completed".to_string(), text, structured)
                    }
                    Err(e) => ("failed".to_string(), Some(e.message.to_string()), None),
                }
            } else {
                (
                    "completed".to_string(),
                    Some("(result type unknown)".to_string()),
                    None,
                )
            }
        }
        Err(e) => {
            let status = if e.to_string().contains("cancelled") {
                "cancelled"
            } else {
                "failed"
            };
            (status.to_string(), Some(e.to_string()), None)
        }
    }
}

async fn handle_cancel(server: &MiraServer, task_id: &str) -> Result<Json<TasksOutput>, MiraError> {
    let mut proc = server.processor.lock().await;
    if proc.cancel_task(task_id) {
        Ok(Json(TasksOutput {
            action: "cancel".to_string(),
            message: format!("Task {} cancelled", task_id),
            data: None,
        }))
    } else {
        Err(MiraError::InvalidInput(format!(
            "Task '{}' not found or already completed. Use session(action=\"tasks_list\") to see current tasks.",
            task_id
        )))
    }
}
