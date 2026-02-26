// crates/mira-server/src/hooks/stop.rs
// Stop hook handler - checks goal progress, snapshots tasks, and saves session state

use crate::hooks::{read_hook_input, write_hook_output};
use crate::ipc::client::HookClient;
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};

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

    tracing::debug!(
        session = %truncate_at_boundary(&stop_input.session_id, 8),
        stop_hook_active = stop_input.stop_hook_active,
        "Stop hook triggered"
    );

    // Don't create infinite loops - if stop hook is already active, just allow stop
    if stop_input.stop_hook_active {
        tracing::debug!("Stop hook already active, allowing stop");
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    // Connect to MCP server (or fall back to direct DB)
    let mut client = HookClient::connect().await;

    // Get current project
    let sid = Some(stop_input.session_id.as_str()).filter(|s| !s.is_empty());
    let Some((project_id, _)) = client.resolve_project(None, sid).await else {
        // No active project, just allow stop
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Check for in-progress goals
    let goal_lines = client.get_active_goals(project_id, 5).await;

    // Build output
    let output = serde_json::json!({});

    if !goal_lines.is_empty() {
        // Log active goals to stderr (Stop hook doesn't support additionalContext)
        tracing::warn!(
            count = goal_lines.len(),
            "[mira] Active goal(s) — remember to update progress"
        );
        for line in &goal_lines {
            tracing::warn!("  {}", line);
        }
    }

    // Log brief injection stats for this session
    if !stop_input.session_id.is_empty() {
        let db_path = crate::hooks::get_db_path();
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            if let Ok(stats) = crate::db::injection::get_injection_stats_for_session(
                &conn,
                &stop_input.session_id,
            ) {
                if stats.total_injections > 0 {
                    let avg_latency = stats
                        .avg_latency_ms
                        .map(|ms| format!(", avg {:.0}ms", ms))
                        .unwrap_or_default();
                    tracing::warn!(
                        "[mira] Session injection stats: {} injections, {} chars total ({} deduped, {} cached{})",
                        stats.total_injections,
                        stats.total_chars,
                        stats.total_deduped,
                        stats.total_cached,
                        avg_latency,
                    );
                }
            }
        }
    }

    // Build session summary, save snapshot, and close the session
    client.close_session(&stop_input.session_id).await;

    // Snapshot native Claude Code tasks
    snapshot_tasks(&mut client, project_id, &stop_input.session_id, false).await;

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

    tracing::debug!(
        session = %truncate_at_boundary(session_id, 8),
        "SessionEnd hook triggered"
    );

    // Connect to MCP server (or fall back to direct DB)
    let mut client = HookClient::connect().await;

    // Deactivate team session if we're in a team
    if !session_id.is_empty() {
        if let Some(membership) = client.get_team_membership(session_id).await {
            client.deactivate_team_session(session_id).await;
            tracing::debug!(
                team = %membership.team_name,
                "Deactivated team session"
            );
        }
        // Clean up per-session team file
        crate::hooks::session::cleanup_team_file(session_id);
    }

    // Close the session (builds summary, saves snapshot, updates status)
    if !session_id.is_empty() {
        client.close_session(session_id).await;

        tracing::debug!(
            session = %truncate_at_boundary(session_id, 8),
            "SessionEnd closed session"
        );
    }

    // Get current project (id and path) — must happen BEFORE per-session cleanup
    // so resolve_project can still read the per-session cwd file
    let end_sid = Some(session_id).filter(|s| !s.is_empty());
    let project_id = client
        .resolve_project(None, end_sid)
        .await
        .map(|(id, _)| id);

    if let Some(project_id) = project_id {
        // Snapshot native Claude Code tasks
        snapshot_tasks(&mut client, project_id, session_id, true).await;
    }

    // Clean up per-session directory AFTER all project resolution is done
    if !session_id.is_empty() {
        crate::hooks::session::cleanup_per_session_dir(session_id);
    }

    write_hook_output(&serde_json::json!({}));
    Ok(())
}

/// Snapshot Claude Code's native task files into Mira's database.
/// Always approves on any error — never blocks session end due to task snapshotting failure.
async fn snapshot_tasks(
    client: &mut HookClient,
    project_id: i64,
    session_id: &str,
    is_session_end: bool,
) {
    let task_list_dir = match crate::tasks::find_current_task_list() {
        Some(dir) => dir,
        None => {
            tracing::debug!("No native task list found, skipping snapshot");
            return;
        }
    };

    let list_id = crate::tasks::task_list_id(&task_list_dir).unwrap_or_default();

    let tasks = match crate::tasks::read_task_list(&task_list_dir) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Failed to read native tasks: {}", e);
            return;
        }
    };

    if tasks.is_empty() {
        return;
    }

    let sid = if session_id.is_empty() {
        None
    } else {
        Some(session_id)
    };

    client
        .snapshot_tasks(project_id, &list_id, sid, &tasks, is_session_end)
        .await;
}

/// Build a session summary from tool history or behavior log, whichever is richer.
pub(crate) fn build_session_summary(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Option<String> {
    // Get stats from both sources and pick the richer one.
    // Compare total event counts (including file_access for behavior) to match
    // the background worker's line-count comparison in session_summaries.rs.
    let (tool_count, top_tools) = match crate::db::get_session_stats_sync(conn, session_id) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Session stats query failed for {}: {}", session_id, e);
            (0, Vec::new())
        }
    };
    let (behavior_count, behavior_tools) = super::get_behavior_tool_stats_sync(conn, session_id);

    // Count all behavior events (tool_use + file_access) for fair comparison,
    // since behavior summaries include both event types.
    // Cap both at 50 to match the LIMIT 50 in get_session_tool_summary_sync
    // and get_session_behavior_summary_sync, aligning with the background
    // worker's line-count comparison.
    let behavior_total: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM session_behavior_log WHERE session_id = ? AND event_type IN ('tool_use', 'file_access')",
            [session_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .min(50) as usize;

    let use_behavior = behavior_total > tool_count.min(50);

    let (count, tool_names, files_modified) = if use_behavior {
        let names: Vec<String> = behavior_tools.iter().map(|(n, _)| n.clone()).collect();
        let files = super::get_behavior_modified_files_sync(conn, session_id);
        (behavior_count as usize, names, files)
    } else if tool_count > 0 {
        let files = super::get_session_modified_files_sync(conn, session_id);
        (tool_count, top_tools, files)
    } else {
        return None;
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

    if !tool_names.is_empty() {
        parts.push(format!("{} tool calls ({})", count, tool_names.join(", ")));
    } else {
        parts.push(format!("{} tool calls", count));
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

        tracing::debug!(
            tools = behavior_count,
            files_modified = files_modified.len(),
            "Session snapshot saved from behavior log"
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

    tracing::debug!(
        tools = tool_count,
        files_modified = files_modified.len(),
        "Session snapshot saved"
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

    // ── regression tests ─────────────────────────────────────────────────

    #[test]
    fn behavior_tool_stats_counts_all_tools_not_just_top_5() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/many-tools-proj");
        seed_session(&conn, "many-tools-sess", pid, "active");

        // Seed 7 distinct tools (> 5 limit that was previously applied)
        for (i, tool) in [
            "Read",
            "Edit",
            "Write",
            "Bash",
            "Glob",
            "Grep",
            "NotebookEdit",
        ]
        .iter()
        .enumerate()
        {
            seed_behavior_tool_use(&conn, "many-tools-sess", pid, tool, i as i64 + 1);
        }
        // Add extra calls to some tools so total > 7
        seed_behavior_tool_use(&conn, "many-tools-sess", pid, "Read", 8);
        seed_behavior_tool_use(&conn, "many-tools-sess", pid, "Edit", 9);

        let (total, top_tools): (i64, Vec<(String, i64)>) =
            crate::hooks::get_behavior_tool_stats_sync(&conn, "many-tools-sess");

        assert_eq!(total, 9, "total should count ALL 9 events across 7 tools");
        assert_eq!(top_tools.len(), 5, "should only return top 5 tools");
        // Top tools should have actual counts
        for (name, count) in &top_tools {
            assert!(*count > 0, "tool {name} should have count > 0");
        }
    }

    #[test]
    fn build_summary_prefers_richer_behavior_log_over_sparse_tool_history() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/mixed-proj");
        seed_session(&conn, "mixed-sess", pid, "active");

        // Sparse tool_history: 1 entry
        seed_tool_history(&conn, "mixed-sess", "Read", r#"{"file_path":"f.rs"}"#, "ok");

        // Rich behavior log: 5 entries
        for i in 0..5 {
            seed_behavior_tool_use(&conn, "mixed-sess", pid, "Edit", i + 1);
        }
        seed_behavior_file_access(
            &conn,
            "mixed-sess",
            pid,
            "Edit",
            "/tmp/mixed-proj/main.rs",
            6,
        );

        let summary = build_session_summary(&conn, "mixed-sess");
        assert!(summary.is_some(), "should produce a summary");
        let s = summary.unwrap();
        assert!(
            s.contains("5 tool calls"),
            "should use behavior_log count (5), not tool_history (1), got: {s}"
        );
    }

    #[test]
    fn build_summary_file_access_events_tip_source_selection() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/file-access-proj");
        seed_session(&conn, "file-access-sess", pid, "active");

        // tool_history: 3 entries
        seed_tool_history(
            &conn,
            "file-access-sess",
            "Read",
            r#"{"file_path":"a.rs"}"#,
            "ok",
        );
        seed_tool_history(
            &conn,
            "file-access-sess",
            "Read",
            r#"{"file_path":"b.rs"}"#,
            "ok",
        );
        seed_tool_history(
            &conn,
            "file-access-sess",
            "Grep",
            r#"{"pattern":"foo"}"#,
            "ok",
        );

        // behavior log: 2 tool_use + 3 file_access = 5 total events
        // tool_use alone (2) would NOT beat tool_history (3), but total events (5) do
        seed_behavior_tool_use(&conn, "file-access-sess", pid, "Edit", 1);
        seed_behavior_tool_use(&conn, "file-access-sess", pid, "Edit", 2);
        seed_behavior_file_access(
            &conn,
            "file-access-sess",
            pid,
            "Edit",
            "/tmp/file-access-proj/a.rs",
            3,
        );
        seed_behavior_file_access(
            &conn,
            "file-access-sess",
            pid,
            "Edit",
            "/tmp/file-access-proj/b.rs",
            4,
        );
        seed_behavior_file_access(
            &conn,
            "file-access-sess",
            pid,
            "Write",
            "/tmp/file-access-proj/c.rs",
            5,
        );

        let summary = build_session_summary(&conn, "file-access-sess");
        assert!(summary.is_some(), "should produce a summary");
        let s = summary.unwrap();
        // Should pick behavior_log (5 total events > 3 tool_history) and report
        // the behavior tool_use count (2), not tool_history count (3)
        assert!(
            s.contains("2 tool calls"),
            "should use behavior_log (2 tool calls from 5 total events), not tool_history (3), got: {s}"
        );
    }

    #[test]
    fn snapshot_behavior_log_has_real_counts() {
        let conn = setup_test_connection();
        let pid = seed_project(&conn, "/tmp/snap-counts-proj");
        seed_session(&conn, "snap-counts-sess", pid, "active");

        // No tool_history — behavior log with known counts
        seed_behavior_tool_use(&conn, "snap-counts-sess", pid, "Edit", 1);
        seed_behavior_tool_use(&conn, "snap-counts-sess", pid, "Edit", 2);
        seed_behavior_tool_use(&conn, "snap-counts-sess", pid, "Edit", 3);
        seed_behavior_tool_use(&conn, "snap-counts-sess", pid, "Read", 4);

        save_session_snapshot(&conn, "snap-counts-sess").unwrap();

        let snapshot_str: String = conn
            .query_row(
                "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
                ["snap-counts-sess"],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot_str).unwrap();

        let top_tools = snapshot["top_tools"].as_array().unwrap();
        // Edit should have count 3, Read should have count 1
        let edit_entry = top_tools.iter().find(|t| t["name"] == "Edit").unwrap();
        assert_eq!(
            edit_entry["count"], 3,
            "Edit count should be 3, not 0, got: {edit_entry}"
        );
        let read_entry = top_tools.iter().find(|t| t["name"] == "Read").unwrap();
        assert_eq!(
            read_entry["count"], 1,
            "Read count should be 1, not 0, got: {read_entry}"
        );
    }
}
