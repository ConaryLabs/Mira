// crates/mira-server/src/hooks/stop.rs
// Stop hook handler - checks goal progress, snapshots tasks, and saves session state

use crate::db::pool::DatabasePool;
use crate::hooks::{get_db_path, read_hook_input, resolve_project_id, write_hook_output};
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
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
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
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
    let pool = match DatabasePool::open(&db_path).await {
        Ok(p) => Arc::new(p),
        Err(_) => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Get current project
    let Some(project_id) = resolve_project_id(&pool).await else {
        // No active project, just allow stop
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Check for in-progress goals
    let goal_lines = super::format_active_goals(&pool, project_id, 5).await;

    // Build output
    let output = serde_json::json!({});

    if !goal_lines.is_empty() {
        // Log active goals to stderr (Stop hook doesn't support additionalContext)
        eprintln!("[mira] {} active goal(s) found:", goal_lines.len());
        for line in &goal_lines {
            eprintln!("[mira]   {}", line);
        }
    }

    // Build session summary, save snapshot, and close the session
    {
        let session_id = stop_input.session_id.clone();
        pool.try_interact_warn("session close", move |conn| {
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
                if let Err(e) = crate::db::close_session_sync(conn, &session_id, summary.as_deref())
                {
                    eprintln!("  Warning: failed to close session: {e}");
                }
                eprintln!(
                    "[mira] Closed session {}",
                    truncate_at_boundary(&session_id, 8)
                );
            }
            Ok(())
        })
        .await;
    }

    // Snapshot native Claude Code tasks
    snapshot_tasks(&pool, project_id, &stop_input.session_id, false).await;

    // Auto-export ranked memories to CLAUDE.local.md
    {
        let pid = project_id;
        pool.try_interact_warn("CLAUDE.local.md export", move |conn| {
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
            Ok(())
        })
        .await;
    }

    write_hook_output(&output);
    Ok(())
}

/// Run SessionEnd hook (fires on user interrupt — always approve, just snapshot)
pub async fn run_session_end() -> Result<()> {
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
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

            let sid = session_id.to_string();
            if let Err(e) = pool
                .run(move |conn| crate::db::deactivate_team_session_sync(conn, &sid))
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

    // Build session summary and close the session (same as Stop hook)
    if !session_id.is_empty() {
        let sid = session_id.to_string();
        pool.try_interact_warn("session end close", move |conn| {
            // Build session summary from stats
            let summary = build_session_summary(conn, &sid);

            // Save structured session snapshot for future resume context
            if let Err(e) = save_session_snapshot(conn, &sid) {
                eprintln!("[mira] SessionEnd snapshot failed: {}", e);
            }

            // Close the session with summary
            if let Err(e) = crate::db::close_session_sync(conn, &sid, summary.as_deref()) {
                eprintln!("  Warning: failed to close session: {e}");
            }
            eprintln!(
                "[mira] SessionEnd closed session {}",
                truncate_at_boundary(&sid, 8)
            );
            Ok(())
        })
        .await;
    }

    // Get current project (id and path)
    let (project_id, project_path) = super::resolve_project(&pool).await;

    if let Some(project_id) = project_id
        && let Some(project_path) = project_path
    {
        // Snapshot native Claude Code tasks
        snapshot_tasks(&pool, project_id, session_id, true).await;

        // Auto-export to Claude Code's auto memory (if feature available)
        let path_clone = project_path.clone();
        pool.try_interact_warn("auto memory export", move |conn| {
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
            Ok(())
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
        // Fallback: check behavior log for native Claude Code tool usage
        let (behavior_count, behavior_tools) =
            super::get_behavior_tool_stats_sync(conn, session_id);
        if behavior_count == 0 {
            return None;
        }

        // Build summary from behavior log data
        let files_modified = super::get_behavior_modified_files_sync(conn, session_id);
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

        let tool_names: Vec<&str> = behavior_tools.iter().map(|(n, _)| n.as_str()).collect();
        let mut parts: Vec<String> = Vec::new();
        if !tool_names.is_empty() {
            parts.push(format!(
                "{} tool calls ({})",
                behavior_count,
                tool_names.join(", ")
            ));
        } else {
            parts.push(format!("{} tool calls", behavior_count));
        }
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
        if let Some(dur) = duration {
            parts.push(format!("Duration: {}", dur));
        }
        return Some(parts.join(". "));
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

/// Save a structured session snapshot for future resume context.
///
/// Captures tool usage counts, top tools, and modified files as JSON
/// in the session_snapshots table.
pub(crate) fn save_session_snapshot(conn: &rusqlite::Connection, session_id: &str) -> Result<()> {
    // Get tool count and top tools
    let (tool_count, top_tools) =
        crate::db::get_session_stats_sync(conn, session_id).unwrap_or((0, Vec::new()));

    if tool_count == 0 {
        // Fallback: check behavior log for native Claude Code tool usage
        let (behavior_count, behavior_tools) =
            super::get_behavior_tool_stats_sync(conn, session_id);
        if behavior_count == 0 {
            return Ok(());
        }

        let files_modified = super::get_behavior_modified_files_sync(conn, session_id);
        let top_tools_json: Vec<serde_json::Value> = behavior_tools
            .iter()
            .map(|(name, count)| serde_json::json!({"name": name, "count": count}))
            .collect();
        let top_tool_names: Vec<&str> = behavior_tools.iter().map(|(n, _)| n.as_str()).collect();

        // Check for existing compaction context
        let existing_compaction: Option<serde_json::Value> = conn
            .query_row(
                "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|json_str| serde_json::from_str::<serde_json::Value>(&json_str).ok())
            .and_then(|snap| snap.get("compaction_context").cloned());

        let mut snapshot = serde_json::json!({
            "tool_count": behavior_count,
            "top_tools": top_tools_json,
            "top_tool_names": top_tool_names,
            "files_modified": files_modified,
            "source": "behavior_log",
        });

        if let Some(compaction) = existing_compaction {
            snapshot["compaction_context"] = compaction;
        }

        let snapshot_str = serde_json::to_string(&snapshot)?;
        conn.execute(
            "INSERT INTO session_snapshots (session_id, snapshot, created_at)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(session_id) DO UPDATE SET snapshot = ?2, created_at = datetime('now')",
            rusqlite::params![session_id, snapshot_str],
        )?;

        eprintln!(
            "[mira] Session snapshot saved from behavior log ({} tools, {} files modified)",
            behavior_count,
            files_modified.len()
        );
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

    // Check if a compaction_context was stored by PreCompact hook
    let existing_compaction: Option<serde_json::Value> = conn
        .query_row(
            "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
            [session_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json_str| serde_json::from_str::<serde_json::Value>(&json_str).ok())
        .and_then(|snap| snap.get("compaction_context").cloned());

    let mut snapshot = serde_json::json!({
        "tool_count": tool_count,
        "top_tools": top_tools_json,
        "top_tool_names": top_tools,
        "files_modified": files_modified,
    });

    // Preserve compaction_context from PreCompact hook
    if let Some(compaction) = existing_compaction {
        snapshot["compaction_context"] = compaction;
    }

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

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::{
        seed_goal, seed_session, seed_tool_history, setup_test_connection,
    };

    // ── StopInput::from_json ──────────────────────────────────────────────

    #[test]
    fn stop_input_defaults_on_empty_json() {
        let input = StopInput::from_json(&serde_json::json!({}));
        assert!(input.session_id.is_empty());
        assert!(!input.stop_hook_active);
    }

    #[test]
    fn stop_input_parses_valid_fields() {
        let input = StopInput::from_json(&serde_json::json!({
            "session_id": "sess-1",
            "stop_hook_active": true
        }));
        assert_eq!(input.session_id, "sess-1");
        assert!(input.stop_hook_active);
    }

    #[test]
    fn stop_input_ignores_wrong_types() {
        let input = StopInput::from_json(&serde_json::json!({
            "session_id": 123,
            "stop_hook_active": "not-a-bool"
        }));
        // Wrong type for session_id falls back to ""
        assert!(input.session_id.is_empty());
        // Wrong type for stop_hook_active falls back to false
        assert!(!input.stop_hook_active);
    }

    #[test]
    fn stop_input_handles_partial_fields() {
        let input = StopInput::from_json(&serde_json::json!({
            "session_id": "sess-2"
        }));
        assert_eq!(input.session_id, "sess-2");
        assert!(!input.stop_hook_active);
    }

    // ── build_session_summary ─────────────────────────────────────────────

    #[test]
    fn build_summary_no_tools_returns_none() {
        let conn = setup_test_connection();
        crate::db::get_or_create_project_sync(&conn, "/tmp/empty-proj", None).unwrap();
        seed_session(&conn, "empty-sess", 1, "active");

        let summary = build_session_summary(&conn, "empty-sess");
        // Returns None when tool_count == 0
        assert!(summary.is_none());
    }

    #[test]
    fn build_summary_nonexistent_session() {
        let conn = setup_test_connection();
        let summary = build_session_summary(&conn, "no-such-session");
        // Should return None — not panic
        assert!(summary.is_none());
    }

    #[test]
    fn build_summary_with_tools() {
        let conn = setup_test_connection();
        crate::db::get_or_create_project_sync(&conn, "/tmp/tool-proj", None).unwrap();
        seed_session(&conn, "tool-sess", 1, "active");
        seed_tool_history(
            &conn,
            "tool-sess",
            "Read",
            r#"{"file_path":"/tmp/tool-proj/foo.rs"}"#,
            "contents",
        );
        seed_tool_history(
            &conn,
            "tool-sess",
            "Read",
            r#"{"file_path":"/tmp/tool-proj/bar.rs"}"#,
            "contents",
        );
        seed_tool_history(
            &conn,
            "tool-sess",
            "Edit",
            r#"{"file_path":"/tmp/tool-proj/foo.rs"}"#,
            "ok",
        );

        let summary = build_session_summary(&conn, "tool-sess");
        assert!(summary.is_some());
        let s = summary.unwrap();
        assert!(!s.is_empty());
    }

    #[test]
    fn build_summary_with_modified_files() {
        let conn = setup_test_connection();
        crate::db::get_or_create_project_sync(&conn, "/tmp/mod-proj", None).unwrap();
        seed_session(&conn, "mod-sess", 1, "active");
        seed_tool_history(
            &conn,
            "mod-sess",
            "Write",
            r#"{"file_path":"/tmp/mod-proj/new.rs"}"#,
            "ok",
        );

        let summary = build_session_summary(&conn, "mod-sess");
        assert!(summary.is_some());
    }

    #[test]
    fn build_summary_many_files_truncates() {
        let conn = setup_test_connection();
        crate::db::get_or_create_project_sync(&conn, "/tmp/big-proj", None).unwrap();
        seed_session(&conn, "big-sess", 1, "active");
        for i in 0..30 {
            let args = format!(r#"{{"file_path":"/tmp/big-proj/file_{}.rs"}}"#, i);
            seed_tool_history(&conn, "big-sess", "Edit", &args, "ok");
        }

        let summary = build_session_summary(&conn, "big-sess");
        assert!(summary.is_some());
        let s = summary.unwrap();
        assert!(s.len() < 10_000);
    }

    // ── format_active_goals_sync (shared) ─────────────────────────────────

    #[test]
    fn goals_empty_when_none_exist() {
        let conn = setup_test_connection();
        let goals = crate::hooks::format_active_goals_sync(&conn, 1, 5);
        assert!(goals.is_empty());
    }

    /// Helper to create a project in the test DB, returning its ID.
    fn seed_project(conn: &rusqlite::Connection, path: &str) -> i64 {
        crate::db::get_or_create_project_sync(conn, path, None)
            .unwrap()
            .0
    }

    #[test]
    fn goals_filters_by_status() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/goal-test");
        seed_goal(&conn, pid, "Active Goal", "in_progress", 50);
        seed_goal(&conn, pid, "Done Goal", "completed", 100);
        seed_goal(&conn, pid, "Blocked Goal", "blocked", 20);

        let goals = crate::hooks::format_active_goals_sync(&conn, pid, 5);
        assert!(!goals.is_empty());
    }

    #[test]
    fn goals_with_milestones() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/ms-test");
        seed_goal(&conn, pid, "Goal With MS", "in_progress", 30);

        let goals = crate::hooks::format_active_goals_sync(&conn, pid, 5);
        assert!(!goals.is_empty());
    }

    #[test]
    fn goals_limits_to_five() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/limit-test");
        for i in 0..8 {
            seed_goal(&conn, pid, &format!("Goal {}", i), "in_progress", 10);
        }

        let goals = crate::hooks::format_active_goals_sync(&conn, pid, 5);
        assert!(goals.len() <= 5);
    }

    #[test]
    fn goals_isolates_by_project() {
        let conn = setup_test_connection();
        let pid1 = seed_project(&conn, "/tmp/proj1");
        let pid2 = seed_project(&conn, "/tmp/proj2");
        seed_goal(&conn, pid1, "Project 1 Goal", "in_progress", 40);
        seed_goal(&conn, pid2, "Project 2 Goal", "in_progress", 60);

        let goals = crate::hooks::format_active_goals_sync(&conn, pid1, 5);
        for g in &goals {
            assert!(!g.contains("Project 2"));
        }
    }

    // ── save_session_snapshot preserves compaction_context ────────────────

    #[test]
    fn snapshot_preserves_compaction_context() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/compact-test");
        seed_session(&conn, "compact-sess", pid, "active");
        seed_tool_history(
            &conn,
            "compact-sess",
            "Read",
            r#"{"file_path":"/tmp/f.rs"}"#,
            "ok",
        );

        // Simulate PreCompact having stored a compaction_context
        let precompact_snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": ["chose builder pattern"],
                "active_work": ["working on migration"],
                "issues": [],
                "pending_tasks": ["add validation"]
            }
        });
        conn.execute(
            "INSERT INTO session_snapshots (session_id, snapshot, created_at)
             VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![
                "compact-sess",
                serde_json::to_string(&precompact_snapshot).unwrap()
            ],
        )
        .unwrap();

        // Now run save_session_snapshot (simulating the Stop hook)
        save_session_snapshot(&conn, "compact-sess").unwrap();

        // Verify compaction_context is preserved
        let snapshot_str: String = conn
            .query_row(
                "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
                ["compact-sess"],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot_str).unwrap();

        // Should have both tool data and compaction_context
        assert!(snapshot.get("tool_count").is_some(), "missing tool_count");
        assert!(
            snapshot.get("compaction_context").is_some(),
            "compaction_context was lost during stop hook snapshot"
        );
        let cc = snapshot.get("compaction_context").unwrap();
        assert!(
            cc.get("decisions")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false),
            "decisions were lost"
        );
    }

    #[test]
    fn snapshot_without_precompact_has_no_compaction_context() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/no-compact-test");
        seed_session(&conn, "no-compact-sess", pid, "active");
        seed_tool_history(
            &conn,
            "no-compact-sess",
            "Read",
            r#"{"file_path":"/tmp/f.rs"}"#,
            "ok",
        );

        save_session_snapshot(&conn, "no-compact-sess").unwrap();

        let snapshot_str: String = conn
            .query_row(
                "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
                ["no-compact-sess"],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot_str).unwrap();

        assert!(snapshot.get("tool_count").is_some());
        assert!(
            snapshot.get("compaction_context").is_none(),
            "compaction_context should not exist when PreCompact didn't run"
        );
    }

    // ── behavior log fallback ───────────────────────────────────────────

    /// Seed behavior log tool_use events (simulates native Claude Code tool calls).
    fn seed_behavior_tool_use(
        conn: &rusqlite::Connection,
        session_id: &str,
        project_id: i64,
        tool_name: &str,
        seq: i64,
    ) {
        conn.execute(
            "INSERT INTO session_behavior_log (session_id, project_id, event_type, event_data, sequence_position)
             VALUES (?, ?, 'tool_use', ?, ?)",
            rusqlite::params![
                session_id,
                project_id,
                serde_json::json!({"tool_name": tool_name}).to_string(),
                seq,
            ],
        )
        .unwrap();
    }

    /// Seed behavior log file_access events (simulates Write/Edit file modifications).
    fn seed_behavior_file_access(
        conn: &rusqlite::Connection,
        session_id: &str,
        project_id: i64,
        action: &str,
        file_path: &str,
        seq: i64,
    ) {
        conn.execute(
            "INSERT INTO session_behavior_log (session_id, project_id, event_type, event_data, sequence_position)
             VALUES (?, ?, 'file_access', ?, ?)",
            rusqlite::params![
                session_id,
                project_id,
                serde_json::json!({"action": action, "file_path": file_path}).to_string(),
                seq,
            ],
        )
        .unwrap();
    }

    #[test]
    fn build_summary_behavior_log_fallback() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/behavior-proj");
        seed_session(&conn, "behavior-sess", pid, "active");

        // No tool_history entries — only behavior log
        seed_behavior_tool_use(&conn, "behavior-sess", pid, "Edit", 1);
        seed_behavior_tool_use(&conn, "behavior-sess", pid, "Edit", 2);
        seed_behavior_tool_use(&conn, "behavior-sess", pid, "Read", 3);
        seed_behavior_file_access(
            &conn,
            "behavior-sess",
            pid,
            "Edit",
            "/tmp/behavior-proj/main.rs",
            4,
        );

        let summary = build_session_summary(&conn, "behavior-sess");
        assert!(
            summary.is_some(),
            "should produce summary from behavior log"
        );
        let s = summary.unwrap();
        assert!(
            s.contains("3 tool calls"),
            "should report 3 tool calls, got: {s}"
        );
        assert!(s.contains("Edit"), "should mention Edit tool, got: {s}");
        assert!(
            s.contains("main.rs"),
            "should mention modified file, got: {s}"
        );
    }

    #[test]
    fn build_summary_returns_none_when_both_empty() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/empty-both-proj");
        seed_session(&conn, "empty-both-sess", pid, "active");

        // No tool_history AND no behavior log
        let summary = build_session_summary(&conn, "empty-both-sess");
        assert!(
            summary.is_none(),
            "should return None when both sources are empty"
        );
    }

    #[test]
    fn snapshot_behavior_log_fallback() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/snap-behavior-proj");
        seed_session(&conn, "snap-behavior-sess", pid, "active");

        // No tool_history — only behavior log
        seed_behavior_tool_use(&conn, "snap-behavior-sess", pid, "Write", 1);
        seed_behavior_tool_use(&conn, "snap-behavior-sess", pid, "Bash", 2);
        seed_behavior_file_access(
            &conn,
            "snap-behavior-sess",
            pid,
            "Write",
            "/tmp/snap-behavior-proj/new_file.rs",
            3,
        );

        save_session_snapshot(&conn, "snap-behavior-sess").unwrap();

        let snapshot_str: String = conn
            .query_row(
                "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
                ["snap-behavior-sess"],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot_str).unwrap();

        assert_eq!(
            snapshot["tool_count"], 2,
            "should count 2 behavior tool calls"
        );
        assert_eq!(
            snapshot["source"], "behavior_log",
            "should mark source as behavior_log"
        );
        let top_names = snapshot["top_tool_names"].as_array().unwrap();
        assert!(!top_names.is_empty(), "should have top tool names");
        let files = snapshot["files_modified"].as_array().unwrap();
        assert_eq!(files.len(), 1, "should have 1 modified file");
        assert_eq!(files[0], "/tmp/snap-behavior-proj/new_file.rs");
    }

    #[test]
    fn snapshot_behavior_log_no_activity_skips() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/snap-empty-proj");
        seed_session(&conn, "snap-empty-sess", pid, "active");

        // No tool_history AND no behavior log
        save_session_snapshot(&conn, "snap-empty-sess").unwrap();

        // Should not create a snapshot row
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_snapshots WHERE session_id = ?",
                ["snap-empty-sess"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "should not create snapshot when no activity");
    }

    #[test]
    fn snapshot_behavior_log_preserves_compaction_context() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/snap-compact-behavior");
        seed_session(&conn, "snap-compact-beh", pid, "active");

        // Pre-seed a compaction context (as if PreCompact ran)
        let precompact = serde_json::json!({
            "compaction_context": {
                "decisions": ["use behavior log fallback"],
                "active_work": [],
                "issues": [],
                "pending_tasks": []
            }
        });
        conn.execute(
            "INSERT INTO session_snapshots (session_id, snapshot, created_at)
             VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![
                "snap-compact-beh",
                serde_json::to_string(&precompact).unwrap()
            ],
        )
        .unwrap();

        // Only behavior log, no tool_history
        seed_behavior_tool_use(&conn, "snap-compact-beh", pid, "Read", 1);

        save_session_snapshot(&conn, "snap-compact-beh").unwrap();

        let snapshot_str: String = conn
            .query_row(
                "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
                ["snap-compact-beh"],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot_str).unwrap();

        assert_eq!(snapshot["source"], "behavior_log");
        assert!(
            snapshot.get("compaction_context").is_some(),
            "compaction_context should be preserved in behavior log fallback"
        );
    }
}
