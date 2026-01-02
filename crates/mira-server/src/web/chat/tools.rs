// web/chat/tools.rs
// Tool execution for DeepSeek chat

use mira_types::WsEvent;
use std::time::Instant;
use tracing::{debug, error, info, instrument, warn};

use crate::web::deepseek;
use crate::web::state::AppState;

/// Claude Code usage guide - injected when spawn_claude is first used
const CLAUDE_CODE_GUIDE: &str = r#"## Claude Code Instance Guide (v2.0.76)

You now have a Claude Code instance running. Use `send_to_claude` with this instance_id for follow-up.

### What Claude Code Can Do
- **Read/Write/Edit files** with surgical precision (AST-aware)
- **Run terminal commands** (bash, git, npm, cargo, etc.)
- **Multi-file changes** atomically coordinated
- **Web search/fetch** for documentation lookups

### Effective Follow-ups via send_to_claude
Be specific in your messages:
- "Run the tests and fix any failures"
- "Commit the changes with message 'feat: add X'"
- "Also update the related tests in tests/unit/"
- "Show me the git diff of your changes"

### Claude's Available Tools
- `Read`, `Write`, `Edit`, `Glob`, `Grep` - file operations
- `Bash` - terminal commands (supports background execution)
- `WebFetch`, `WebSearch` - web access
- `Task` - spawn subagents for parallel work

### Tips
- Claude maintains full conversation context
- Output streams to UI in real-time
- Instance persists until killed or task complete
- Multiple instances can run in parallel
"#;

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

                match execute_recall(state, query, limit).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "search_code" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(10);

                match execute_code_search(state, query, limit).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "list_tasks" => {
                match execute_list_tasks(state).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "list_goals" => {
                match execute_list_goals(state).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "spawn_claude" => {
                let initial_prompt = args
                    .get("initial_prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let working_dir = args
                    .get("working_directory")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| {
                        // Use project path if available
                        futures::executor::block_on(state.get_project())
                            .map(|p| p.path)
                    })
                    .unwrap_or_else(|| ".".to_string());

                match state
                    .claude_manager
                    .spawn(working_dir, Some(initial_prompt.to_string()))
                    .await
                {
                    Ok(id) => format!(
                        "Claude instance started with ID: {}\n\n{}",
                        id, CLAUDE_CODE_GUIDE
                    ),
                    Err(e) => format!("Error spawning Claude: {}", e),
                }
            }
            "send_to_claude" => {
                let instance_id = args
                    .get("instance_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");

                match state.claude_manager.send_input(instance_id, message).await {
                    Ok(_) => "Message sent to Claude".to_string(),
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

        // Broadcast tool result
        state.broadcast(WsEvent::ToolResult {
            tool_name: tc.function.name.clone(),
            result: result.clone(),
            success,
            call_id: tc.id.clone(),
            duration_ms,
        });

        results.push((tc.id.clone(), result));
    }

    results
}

async fn execute_recall(state: &AppState, query: &str, limit: i64) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let project = state.get_project().await;

    // Add project context header if project is set
    let context_header = match &project {
        Some(p) => format!(
            "[Project: {} @ {}]\n\n",
            p.name.as_deref().unwrap_or("Unknown"),
            p.path
        ),
        None => String::new(),
    };

    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let conn = state.db.conn();

            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let mut stmt = conn.prepare(
                "SELECT f.content FROM memory_facts f
                 JOIN vec_memory v ON f.id = v.fact_id
                 WHERE (f.project_id = ?1 OR ?1 IS NULL)
                 ORDER BY vec_distance_cosine(v.embedding, ?2)
                 LIMIT ?3",
            )?;

            let memories: Vec<String> = stmt
                .query_map(rusqlite::params![project_id, embedding_bytes, limit], |row| {
                    row.get(0)
                })?
                .filter_map(|r| r.ok())
                .collect();

            if !memories.is_empty() {
                return Ok(format!(
                    "{}Found {} memories:\n{}",
                    context_header,
                    memories.len(),
                    memories.join("\n---\n")
                ));
            }
        }
    }

    Ok(format!("{}No memories found", context_header))
}

async fn execute_code_search(state: &AppState, query: &str, limit: i64) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let project = state.get_project().await;

    // Add project context header if project is set
    let context_header = match &project {
        Some(p) => format!(
            "[Project: {} @ {}]\n\n",
            p.name.as_deref().unwrap_or("Unknown"),
            p.path
        ),
        None => String::new(),
    };

    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let conn = state.db.conn();

            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let mut stmt = conn.prepare(
                "SELECT file_path, chunk_content FROM vec_code
                 WHERE project_id = ?1 OR ?1 IS NULL
                 ORDER BY vec_distance_cosine(embedding, ?2)
                 LIMIT ?3",
            )?;

            let results: Vec<String> = stmt
                .query_map(rusqlite::params![project_id, embedding_bytes, limit], |row| {
                    let path: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    Ok(format!("## {}\n```\n{}\n```", path, content))
                })?
                .filter_map(|r| r.ok())
                .collect();

            if !results.is_empty() {
                return Ok(format!(
                    "{}Found {} code matches:\n{}",
                    context_header,
                    results.len(),
                    results.join("\n\n")
                ));
            }
        }
    }

    Ok(format!("{}No code matches found", context_header))
}

async fn execute_list_tasks(state: &AppState) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let mut stmt = conn.prepare(
        "SELECT title, status, priority FROM tasks
         WHERE project_id = ?1 OR ?1 IS NULL
         ORDER BY created_at DESC LIMIT 20",
    )?;

    let tasks: Vec<String> = stmt
        .query_map([project_id], |row| {
            let title: String = row.get(0)?;
            let status: String = row.get(1)?;
            let priority: String = row.get(2)?;
            Ok(format!("- [{}] {} ({})", status, title, priority))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if tasks.is_empty() {
        Ok("No tasks found".to_string())
    } else {
        Ok(format!("Tasks:\n{}", tasks.join("\n")))
    }
}

async fn execute_list_goals(state: &AppState) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let mut stmt = conn.prepare(
        "SELECT title, status, progress_percent FROM goals
         WHERE project_id = ?1 OR ?1 IS NULL
         ORDER BY created_at DESC LIMIT 10",
    )?;

    let goals: Vec<String> = stmt
        .query_map([project_id], |row| {
            let title: String = row.get(0)?;
            let status: String = row.get(1)?;
            let progress: i32 = row.get(2)?;
            Ok(format!("- [{}] {} ({}%)", status, title, progress))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if goals.is_empty() {
        Ok("No goals found".to_string())
    } else {
        Ok(format!("Goals:\n{}", goals.join("\n")))
    }
}
