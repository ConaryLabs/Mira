// crates/mira-server/src/hooks/stop.rs
// Stop hook handler - checks goal progress, snapshots tasks, and saves session state

use crate::db::pool::DatabasePool;
use crate::hooks::{get_db_path, read_hook_input, resolve_project_id, write_hook_output};
use crate::utils::truncate_at_boundary;
use anyhow::Result;
use std::sync::Arc;

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
    let Some(project_id) = resolve_project_id(&pool).await else {
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

    // Build session summary, save snapshot, and close the session
    {
        let pool_clone = pool.clone();
        let session_id = stop_input.session_id.clone();
        let _ = pool_clone
            .interact(move |conn| {
                if let Err(e) = crate::db::set_server_state_sync(
                    conn,
                    "last_stop_time",
                    &chrono::Utc::now().to_rfc3339(),
                ) {
                    eprintln!("  Warning: failed to save server state: {e}");
                }

                // Build session summary from stats
                let summary = if !session_id.is_empty() {
                    build_session_summary(conn, &session_id)
                } else {
                    None
                };

                // Save structured session snapshot for future resume context
                if !session_id.is_empty()
                    && let Err(e) = save_session_snapshot(conn, &session_id)
                {
                    eprintln!("[mira] Session snapshot failed: {}", e);
                }

                // Close the session with summary
                if !session_id.is_empty() {
                    if let Err(e) =
                        crate::db::close_session_sync(conn, &session_id, summary.as_deref())
                    {
                        eprintln!("  Warning: failed to close session: {e}");
                    }
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
                let path = crate::db::get_last_active_project_sync(conn).unwrap_or_else(|e| {
                    tracing::warn!("Failed to get last active project: {e}");
                    None
                });
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

    // Deactivate team session if we're in a team
    if !session_id.is_empty() {
        if let Some(membership) =
            crate::hooks::session::read_team_membership_from_db(&pool, session_id).await
        {
            // Trigger knowledge distillation before deactivating (async, non-blocking)
            let distill_pool = pool.clone();
            let distill_team_id = membership.team_id;
            let distill_project_id = super::resolve_project_id(&pool).await;

            // Run distillation inline (not spawned) so it completes before the hook exits
            match crate::background::knowledge_distillation::distill_team_session(
                &distill_pool,
                distill_team_id,
                distill_project_id,
            )
            .await
            {
                Ok(Some(result)) => {
                    eprintln!(
                        "[mira] Distilled {} finding(s) from team '{}'",
                        result.findings.len(),
                        result.team_name,
                    );
                }
                Ok(None) => {
                    eprintln!("[mira] No findings to distill for team session");
                }
                Err(e) => {
                    eprintln!("[mira] Knowledge distillation failed: {}", e);
                }
            }

            let pool_clone = pool.clone();
            let sid = session_id.to_string();
            if let Err(e) = pool_clone
                .interact(move |conn| {
                    crate::db::deactivate_team_session_sync(conn, &sid)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
            {
                eprintln!("[mira] Failed to deactivate team session: {}", e);
            }
            eprintln!(
                "[mira] Deactivated team session (team: {})",
                membership.team_name
            );
        }
        // Clean up per-session team file
        crate::hooks::session::cleanup_team_file(session_id);
    }

    // Get current project (id and path)
    let (project_id, project_path) = super::resolve_project(&pool).await;

    if let Some(project_id) = project_id
        && let Some(project_path) = project_path
    {
        // Snapshot native Claude Code tasks
        snapshot_tasks(&pool, project_id, session_id, true).await;

        // Auto-export to Claude Code's auto memory (if feature available)
        let pool_clone = pool.clone();
        let path_clone = project_path.clone();
        let _ = pool_clone
            .interact(move |conn| {
                // Only write if auto memory directory exists (non-invasive feature detection)
                if crate::tools::core::claude_local::auto_memory_dir_exists(&path_clone) {
                    match crate::tools::core::claude_local::write_auto_memory_sync(
                        conn,
                        project_id,
                        &path_clone,
                    ) {
                        Ok(count) if count > 0 => {
                            eprintln!("[mira] Auto-exported {} memories to MEMORY.mira.md", count);
                        }
                        Err(e) => {
                            eprintln!("[mira] Auto memory export failed: {}", e);
                        }
                        _ => {}
                    }
                }
                Ok::<_, anyhow::Error>(())
            })
            .await;
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
            let label = if is_session_end { "SessionEnd" } else { "Stop" };
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
    let (tool_count, top_tools) = match crate::db::get_session_stats_sync(conn, session_id) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Session stats query failed for {}: {}", session_id, e);
            return None;
        }
    };

    if tool_count == 0 {
        return None;
    }

    // Get files modified (Write/Edit tool calls)
    let files_modified = super::get_session_modified_files_sync(conn, session_id);

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
        parts.push(format!(
            "{} tool calls ({})",
            tool_count,
            top_tools.join(", ")
        ));
    } else {
        parts.push(format!("{} tool calls", tool_count));
    }

    // Files modified
    if !files_modified.is_empty() {
        let file_names: Vec<&str> = files_modified
            .iter()
            .map(|p| {
                std::path::Path::new(p.as_str())
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(p)
            })
            .collect();
        if file_names.len() <= 3 {
            parts.push(format!("Modified: {}", file_names.join(", ")));
        } else {
            parts.push(format!(
                "Modified: {} (+{} more)",
                file_names[..3].join(", "),
                file_names.len().saturating_sub(3)
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
               (SELECT title FROM milestones
                WHERE goal_id = g.id AND completed_at IS NULL
                ORDER BY id LIMIT 1) as next_milestone
        FROM goals g
        WHERE g.project_id = ? AND g.status = 'in_progress'
        ORDER BY CASE g.priority WHEN 'urgent' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 WHEN 'low' THEN 4 ELSE 5 END, g.created_at DESC
        LIMIT 5
    "#;

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to prepare in-progress goals query: {e}");
            return Vec::new();
        }
    };

    stmt.query_map([project_id], |row| {
        Ok(GoalInfo {
            title: row.get(0)?,
            progress: row.get(1)?,
            next_milestone: row.get(2)?,
        })
    })
    .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
    .unwrap_or_default()
}

/// Save a structured session snapshot for future resume context.
///
/// Captures tool usage counts, top tools, and modified files as JSON
/// in the session_snapshots table.
fn save_session_snapshot(conn: &rusqlite::Connection, session_id: &str) -> Result<()> {
    // Get tool count and top tools
    let (tool_count, top_tools) =
        crate::db::get_session_stats_sync(conn, session_id).unwrap_or((0, Vec::new()));

    if tool_count == 0 {
        return Ok(());
    }

    // Build top_tools with counts
    let top_tools_json: Vec<serde_json::Value> = {
        let sql = r#"
            SELECT tool_name, COUNT(*) as cnt
            FROM tool_history
            WHERE session_id = ?
            GROUP BY tool_name
            ORDER BY cnt DESC
            LIMIT 5
        "#;
        match conn.prepare(sql) {
            Ok(mut stmt) => stmt
                .query_map(rusqlite::params![session_id], |row| {
                    let name: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok(serde_json::json!({"name": name, "count": count}))
                })
                .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
                .unwrap_or_default(),
            Err(e) => {
                tracing::warn!("Failed to prepare top tools query: {e}");
                Vec::new()
            }
        }
    };

    // Get files modified (Write/Edit/NotebookEdit/MultiEdit tool calls)
    let files_modified = super::get_session_modified_files_sync(conn, session_id);

    let snapshot = serde_json::json!({
        "tool_count": tool_count,
        "top_tools": top_tools_json,
        "top_tool_names": top_tools,
        "files_modified": files_modified,
    });

    let snapshot_str = serde_json::to_string(&snapshot)?;

    conn.execute(
        "INSERT INTO session_snapshots (session_id, snapshot, created_at)
         VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(session_id) DO UPDATE SET snapshot = ?2, created_at = datetime('now')",
        rusqlite::params![session_id, snapshot_str],
    )?;

    eprintln!(
        "[mira] Session snapshot saved ({} tools, {} files modified)",
        tool_count,
        files_modified.len()
    );

    Ok(())
}
