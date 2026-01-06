// crates/mira-server/src/web/chat/tools.rs
// Tool execution for DeepSeek chat - delegates to unified tool core

use std::time::Instant;
use tracing::{debug, error, info, instrument, warn};

use crate::web::deepseek;
use crate::web::state::AppState;
use crate::tools::core::{memory, code, project, tasks_goals, web, claude, bash};

/// Execute tool calls and return results
#[instrument(skip(state, tool_calls), fields(tool_count = tool_calls.len()))]
pub async fn execute_tools(
    state: &AppState,
    tool_calls: &[deepseek::ToolCall],
) -> Vec<(String, String)> {
    let mut results = Vec::new();

    for tc in tool_calls {
        let start_time = Instant::now();
        let args: serde_json::Value =
            serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);

        debug!(
            tool = %tc.function.name,
            call_id = %tc.id,
            args = %args,
            "Executing tool"
        );

            let result = match tc.function.name.as_str() {
                "recall_memories" => {
                    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(5);
                    match memory::recall(state, query.to_string(), Some(limit), None, None).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "search_code" => {
                    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(10);
                    match code::search_code(state, query.to_string(), None, Some(limit)).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "find_callers" => {
                    let function_name = args.get("function_name").and_then(|v| v.as_str()).unwrap_or("");
                    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);
                    match code::find_function_callers(state, function_name.to_string(), Some(limit)).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "find_callees" => {
                    let function_name = args.get("function_name").and_then(|v| v.as_str()).unwrap_or("");
                    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);
                    match code::find_function_callees(state, function_name.to_string(), Some(limit)).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "list_tasks" => {
                    match tasks_goals::task(state, "list".to_string(), None, None, None, None, None, None, None).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "list_goals" => {
                    match tasks_goals::goal(state, "list".to_string(), None, None, None, None, None, None, None, None).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "claude_task" => {
                    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("");
                    if task.is_empty() {
                        "Error: task is required".to_string()
                    } else {
                        match claude::claude_task(state, task.to_string()).await {
                            Ok(r) => r,
                            Err(e) => format!("Error: {}", e),
                        }
                    }
                }
                "claude_close" => {
                    match claude::claude_close(state).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "claude_status" => {
                    match claude::claude_status(state).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "discuss" => {
                    let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    match claude::discuss(state, message.to_string()).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "google_search" => {
                    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                    let num_results = args.get("num_results").and_then(|v| v.as_u64()).unwrap_or(5) as i64;
                    match web::google_search(state, query.to_string(), Some(num_results)).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "web_fetch" => {
                    let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    match web::web_fetch(state, url.to_string()).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "research" => {
                    let question = args.get("question").and_then(|v| v.as_str()).unwrap_or("");
                    let depth = args.get("depth").and_then(|v| v.as_str()).unwrap_or("quick");
                    match web::research(state, question.to_string(), Some(depth.to_string())).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "bash" => {
                    let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                    let timeout = args.get("timeout_seconds").and_then(|v| v.as_u64()).unwrap_or(60);
                    let working_dir = args
                        .get("working_directory")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .or_else(|| {
                            futures::executor::block_on(state.get_project()).map(|p| p.path)
                        });
                    match bash::bash(state, command.to_string(), working_dir, Some(timeout)).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "set_project" => {
                    let name_or_path = args.get("name_or_path").and_then(|v| v.as_str()).unwrap_or("");
                    match project::set_project(state, name_or_path.to_string(), None).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                "list_projects" => {
                    match project::list_projects(state).await {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    }
                }
                _ => {
                    warn!(tool = %tc.function.name, "Unknown tool requested");
                    format!("Unknown tool: {}", tc.function.name)
                }
            };

        let duration_ms = start_time.elapsed().as_millis() as u64;
        let success = !result.starts_with("Error");

        if success {
            info!(
                tool = %tc.function.name,
                call_id = %tc.id,
                duration_ms = duration_ms,
                result_len = result.len(),
                "Tool executed successfully"
            );
        } else {
            error!(
                tool = %tc.function.name,
                call_id = %tc.id,
                duration_ms = duration_ms,
                result = %result,
                "Tool execution failed"
            );
        }

        results.push((tc.id.clone(), result));
    }

    results
}
