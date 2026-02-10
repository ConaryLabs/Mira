// crates/mira-server/src/tools/core/tasks.rs
// Fallback "tasks" tool for clients without native MCP task support.
//
// This handler takes &MiraServer directly (not &impl ToolContext) because
// it needs access to the OperationProcessor which is MCP-specific.

use crate::mcp::MiraServer;
use crate::mcp::requests::SessionAction;
use crate::mcp::responses::{
    Json, TaskSummary, TasksData, TasksListData, TasksOutput, TasksStatusData,
};
use crate::utils::truncate;
use rmcp::task_manager::ToolCallTaskResult;

pub async fn handle_tasks(
    server: &MiraServer,
    action: SessionAction,
    task_id: Option<String>,
) -> Result<Json<TasksOutput>, String> {
    match action {
        SessionAction::TasksList => handle_list(server).await,
        SessionAction::TasksGet => {
            let task_id = task_id.ok_or("task_id is required for session(action=tasks_get)")?;
            handle_get(server, &task_id).await
        }
        SessionAction::TasksCancel => {
            let task_id = task_id.ok_or("task_id is required for session(action=tasks_cancel)")?;
            handle_cancel(server, &task_id).await
        }
        _ => Err("Invalid action for tasks handler".into()),
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

async fn handle_list(server: &MiraServer) -> Result<Json<TasksOutput>, String> {
    let mut proc = server.processor.lock().await;
    proc.check_timeouts();

    // Drain channel and collect completed results (this empties the internal buffer)
    let completed = proc.collect_completed_results();

    let running_ids = proc.list_running();
    let mut summaries: Vec<TaskSummary> = Vec::new();

    for id in &running_ids {
        if let Some(desc) = proc.task_descriptor(id) {
            summaries.push(TaskSummary {
                task_id: id.clone(),
                tool_name: desc.name.clone(),
                status: "working".to_string(),
            });
        }
    }

    for result in &completed {
        summaries.push(TaskSummary {
            task_id: result.descriptor.operation_id.clone(),
            tool_name: result.descriptor.name.clone(),
            status: result_status(&result.result).to_string(),
        });
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

async fn handle_get(server: &MiraServer, task_id: &str) -> Result<Json<TasksOutput>, String> {
    let mut proc = server.processor.lock().await;

    // Drain channel first — moves finished tasks from running to completed
    let completed = proc.collect_completed_results();

    // Check completed results first (task may have just finished)
    let position = completed
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

    match position {
        Some(idx) => {
            // Found — extract from vec (we can't put the rest back easily,
            // but completed results are consumed on get, which is the expected behavior)
            let Some(task_result) = completed.into_iter().nth(idx) else {
                return Err(format!("Task '{}' not found", task_id));
            };

            let (status, result_text, result_structured) = match task_result.result {
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
            };

            let message = match &result_text {
                Some(t) => format!("Task {}: {} — {}", task_id, status, truncate(t, 100)),
                None => format!("Task {}: {}", task_id, status),
            };

            Ok(Json(TasksOutput {
                action: "get".to_string(),
                message,
                data: Some(TasksData::Status(TasksStatusData {
                    task_id: task_id.to_string(),
                    status,
                    result_text,
                    result_structured,
                })),
            }))
        }
        None => Err(format!("Task '{}' not found", task_id)),
    }
}

async fn handle_cancel(server: &MiraServer, task_id: &str) -> Result<Json<TasksOutput>, String> {
    let mut proc = server.processor.lock().await;
    if proc.cancel_task(task_id) {
        Ok(Json(TasksOutput {
            action: "cancel".to_string(),
            message: format!("Task {} cancelled", task_id),
            data: None,
        }))
    } else {
        Err(format!("Task '{}' not found or already completed", task_id))
    }
}
