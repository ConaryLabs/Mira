// crates/mira-server/src/hooks/stop.rs
// Stop hook handler - checks goal progress, snapshots tasks, and saves session state

use crate::db::pool::DatabasePool;
use crate::hooks::{read_hook_input, write_hook_output};
use crate::utils::truncate_at_boundary;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

/// Get database path
fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Stop hook input from Claude Code
#[derive(Debug)]
struct StopInput {
    session_id: String,
    stop_hook_active: bool,
}

impl StopInput {
    fn from_json(json: &serde_json::Value) -> Self {
        Self {
            session_id: json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            stop_hook_active: json
                .get("stop_hook_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }
    }
}

/// Run Stop hook
///
/// This hook fires when Claude finishes responding. We can:
/// 1. Check if there are incomplete goals being worked on
/// 2. Save session state
/// 3. Optionally block stopping if goals need completion
pub async fn run() -> Result<()> {
    let input = read_hook_input()?;
    let stop_input = StopInput::from_json(&input);

    eprintln!(
        "[mira] Stop hook triggered (session: {}, stop_hook_active: {})",
        truncate_at_boundary(&stop_input.session_id, 8),
        stop_input.stop_hook_active
    );

    // Don't create infinite loops - if stop hook is already active, just allow stop
    if stop_input.stop_hook_active {
        eprintln!("[mira] Stop hook already active, allowing stop");
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    // Open database
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    // Get current project
    let project_id = {
        let pool_clone = pool.clone();
        let result: Result<Option<i64>, _> = pool_clone
            .interact(move |conn| {
                let path = crate::db::get_last_active_project_sync(conn).ok().flatten();
                let result = if let Some(path) = path {
                    crate::db::get_or_create_project_sync(conn, &path, None)
                        .ok()
                        .map(|(id, _)| id)
                } else {
                    None
                };
                Ok::<_, anyhow::Error>(result)
            })
            .await;
        result.ok().flatten()
    };

    let Some(project_id) = project_id else {
        // No active project, just allow stop
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Check for in-progress goals
    let goals: Vec<GoalInfo> = {
        let pool_clone = pool.clone();
        pool_clone
            .interact(move |conn| Ok::<_, anyhow::Error>(get_in_progress_goals(conn, project_id)))
            .await
            .unwrap_or_default()
    };

    // Build output
    let mut output = serde_json::json!({});

    if !goals.is_empty() {
        // We have active goals - add context but don't block
        let goal_summary: Vec<String> = goals
            .iter()
            .map(|g| {
                format!(
                    "- {} ({}%): {}",
                    g.title,
                    g.progress,
                    g.next_milestone.as_deref().unwrap_or("no milestones")
                )
            })
            .collect();

        let context = format!("Active goals in progress:\n{}", goal_summary.join("\n"));

        // Add as additional context (informational, not blocking)
        output = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "Stop",
                "additionalContext": context
            }
        });

        eprintln!("[mira] {} active goal(s) found", goals.len());
    }

    // Build session summary and close the session
    {
        let pool_clone = pool.clone();
        let session_id = stop_input.session_id.clone();
        let _ = pool_clone
            .interact(move |conn| {
                crate::db::set_server_state_sync(
                    conn,
                    "last_stop_time",
                    &chrono::Utc::now().to_rfc3339(),
                )
                .ok();

                // Build session summary from stats
                let summary = if !session_id.is_empty() {
                    build_session_summary(conn, &session_id)
                } else {
                    None
                };

                // Close the session with summary
                if !session_id.is_empty() {
                    crate::db::close_session_sync(conn, &session_id, summary.as_deref()).ok();
                    eprintln!(
                        "[mira] Closed session {}",
                        truncate_at_boundary(&session_id, 8)
                    );
                }
                Ok::<_, anyhow::Error>(())
            })
            .await;
    }

    // Snapshot native Claude Code tasks
    snapshot_tasks(&pool, project_id, &stop_input.session_id, false).await;

    // Auto-export ranked memories to CLAUDE.local.md
    {
        let pool_clone = pool.clone();
        let pid = project_id;
        let _ = pool_clone
            .interact(move |conn| {
                let path = crate::db::get_last_active_project_sync(conn).ok().flatten();
                if let Some(project_path) = path {
                    match crate::tools::core::claude_local::write_claude_local_md_sync(
                        conn,
                        pid,
                        &project_path,
                    ) {
                        Ok(count) if count > 0 => {
                            eprintln!("[mira] Auto-exported {} memories to CLAUDE.local.md", count);
                        }
                        Err(e) => {
                            eprintln!("[mira] CLAUDE.local.md export failed: {}", e);
                        }
                        _ => {}
                    }
                }
                Ok::<_, anyhow::Error>(())
            })
            .await;
    }

    write_hook_output(&output);
    Ok(())
}

/// Run SessionEnd hook (fires on user interrupt — always approve, just snapshot)
pub async fn run_session_end() -> Result<()> {
    let input = read_hook_input()?;
    let session_id = input
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    eprintln!(
        "[mira] SessionEnd hook triggered (session: {})",
        truncate_at_boundary(session_id, 8),
    );

    // Open database
    let db_path = get_db_path();
    let pool = match DatabasePool::open(&db_path).await {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("[mira] SessionEnd: failed to open DB: {}", e);
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Get current project
    let project_id = {
        let pool_clone = pool.clone();
        let result: Result<Option<i64>, _> = pool_clone
            .interact(move |conn| {
                let path = crate::db::get_last_active_project_sync(conn).ok().flatten();
                let result = if let Some(path) = path {
                    crate::db::get_or_create_project_sync(conn, &path, None)
                        .ok()
                        .map(|(id, _)| id)
                } else {
                    None
                };
                Ok::<_, anyhow::Error>(result)
            })
            .await;
        result.ok().flatten()
    };

    if let Some(project_id) = project_id {
        snapshot_tasks(&pool, project_id, session_id, true).await;
    }

    write_hook_output(&serde_json::json!({}));
    Ok(())
}

/// Snapshot Claude Code's native task files into Mira's database.
/// Always approves on any error — never blocks session end due to task snapshotting failure.
async fn snapshot_tasks(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    session_id: &str,
    is_session_end: bool,
) {
    let task_list_dir = match crate::tasks::find_current_task_list() {
        Some(dir) => dir,
        None => {
            eprintln!("[mira] No native task list found, skipping snapshot");
            return;
        }
    };

    let list_id = crate::tasks::task_list_id(&task_list_dir).unwrap_or_default();

    let tasks = match crate::tasks::read_task_list(&task_list_dir) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[mira] Failed to read native tasks: {}", e);
            return;
        }
    };

    if tasks.is_empty() {
        return;
    }

    let (completed, remaining) = tasks.iter().fold((0usize, 0usize), |(c, r), t| {
        if t.status == "completed" {
            (c + 1, r)
        } else {
            (c, r + 1)
        }
    });

    let pool_clone = pool.clone();
    let sid = if session_id.is_empty() {
        None
    } else {
        Some(session_id.to_string())
    };

    let result = pool_clone
        .interact(move |conn| {
            let count = crate::db::session_tasks::snapshot_native_tasks_sync(
                conn,
                project_id,
                &list_id,
                sid.as_deref(),
                &tasks,
            )?;
            Ok::<usize, anyhow::Error>(count)
        })
        .await;

    match result {
        Ok(count) => {
            let label = if is_session_end {
                "SessionEnd"
            } else {
                "Stop"
            };
            eprintln!(
                "[mira] {} snapshot: {} tasks ({} completed, {} remaining)",
                label, count, completed, remaining,
            );
        }
        Err(e) => {
            eprintln!("[mira] Task snapshot failed: {}", e);
        }
    }
}

/// Build a session summary from tool history
fn build_session_summary(conn: &rusqlite::Connection, session_id: &str) -> Option<String> {
    // Get session stats
    let (tool_count, top_tools) = crate::db::get_session_stats_sync(conn, session_id).ok()?;

    if tool_count == 0 {
        return None;
    }

    // Get files modified (Write/Edit tool calls)
    let files_modified: Vec<String> = {
        let sql = r#"
            SELECT DISTINCT
                json_extract(arguments, '$.file_path') as file_path
            FROM tool_history
            WHERE session_id = ?
              AND tool_name IN ('Write', 'Edit', 'NotebookEdit')
              AND json_extract(arguments, '$.file_path') IS NOT NULL
            LIMIT 10
        "#;
        conn.prepare(sql)
            .ok()
            .and_then(|mut stmt| {
                stmt.query_map([session_id], |row| row.get::<_, String>(0))
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    };

    // Get session duration
    let duration: Option<String> = {
        let sql = r#"
            SELECT
                CAST((julianday(last_activity) - julianday(started_at)) * 24 * 60 AS INTEGER)
            FROM sessions
            WHERE id = ?
        "#;
        conn.query_row(sql, [session_id], |row| row.get::<_, i64>(0))
            .ok()
            .map(|mins| {
                if mins >= 60 {
                    format!("{}h {}m", mins / 60, mins % 60)
                } else {
                    format!("{}m", mins)
                }
            })
    };

    // Build summary
    let mut parts: Vec<String> = Vec::new();

    // Tools used
    if !top_tools.is_empty() {
        parts.push(format!("{} tool calls ({})", tool_count, top_tools.join(", ")));
    } else {
        parts.push(format!("{} tool calls", tool_count));
    }

    // Files modified
    if !files_modified.is_empty() {
        let file_names: Vec<&str> = files_modified
            .iter()
            .map(|p| p.rsplit('/').next().unwrap_or(p))
            .collect();
        if file_names.len() <= 3 {
            parts.push(format!("Modified: {}", file_names.join(", ")));
        } else {
            parts.push(format!(
                "Modified: {} (+{} more)",
                file_names[..3].join(", "),
                file_names.len() - 3
            ));
        }
    }

    // Duration
    if let Some(dur) = duration {
        parts.push(format!("Duration: {}", dur));
    }

    Some(parts.join(". "))
}

/// Goal info for stop hook
struct GoalInfo {
    title: String,
    progress: i32,
    next_milestone: Option<String>,
}

/// Get in-progress goals for a project
fn get_in_progress_goals(conn: &rusqlite::Connection, project_id: i64) -> Vec<GoalInfo> {
    let sql = r#"
        SELECT g.title, g.progress_percent,
               (SELECT title FROM goal_milestones
                WHERE goal_id = g.id AND completed_at IS NULL
                ORDER BY id LIMIT 1) as next_milestone
        FROM goals g
        WHERE g.project_id = ? AND g.status = 'in_progress'
        ORDER BY g.priority DESC, g.created_at DESC
        LIMIT 5
    "#;

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map([project_id], |row| {
        Ok(GoalInfo {
            title: row.get(0)?,
            progress: row.get(1)?,
            next_milestone: row.get(2)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}
