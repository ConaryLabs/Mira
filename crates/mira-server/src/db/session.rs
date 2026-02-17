// db/session.rs
// Session and tool history operations

use rusqlite::{Connection, params};

use crate::utils::{truncate, truncate_at_boundary};

use super::types::{SessionInfo, ToolHistoryEntry};

// ============================================================================
// Sync functions for pool.interact() usage
// ============================================================================

/// Create or update a session (sync version for pool.interact)
pub fn create_session_sync(
    conn: &Connection,
    session_id: &str,
    project_id: Option<i64>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sessions (id, project_id, status, started_at, last_activity)
         VALUES (?1, ?2, 'active', datetime('now'), datetime('now'))
         ON CONFLICT(id) DO UPDATE SET last_activity = datetime('now')",
        params![session_id, project_id],
    )?;
    Ok(())
}

/// Create or update a session with extended fields (source, resumed_from)
/// Properly reactivates completed sessions by setting status='active'
pub fn create_session_ext_sync(
    conn: &Connection,
    session_id: &str,
    project_id: Option<i64>,
    source: Option<&str>,
    resumed_from: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sessions (id, project_id, status, source, resumed_from, started_at, last_activity)
         VALUES (?1, ?2, 'active', ?3, ?4, datetime('now'), datetime('now'))
         ON CONFLICT(id) DO UPDATE SET
            status = 'active',
            last_activity = datetime('now'),
            project_id = COALESCE(excluded.project_id, sessions.project_id),
            source = CASE
                WHEN sessions.status = 'completed' THEN COALESCE(excluded.source, sessions.source)
                ELSE sessions.source
            END,
            resumed_from = COALESCE(excluded.resumed_from, sessions.resumed_from)",
        params![session_id, project_id, source.unwrap_or("startup"), resumed_from],
    )?;
    Ok(())
}

/// Get recent sessions for a project (sync version for pool.interact)
pub fn get_recent_sessions_sync(
    conn: &Connection,
    project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<SessionInfo>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, status, summary, started_at, last_activity, source, resumed_from
         FROM sessions
         WHERE project_id = ?
         ORDER BY last_activity DESC, rowid DESC
         LIMIT ?",
    )?;
    let rows = stmt.query_map(params![project_id, limit as i64], |row| {
        Ok(SessionInfo {
            id: row.get(0)?,
            project_id: row.get(1)?,
            status: row.get(2)?,
            summary: row.get(3)?,
            started_at: row.get(4)?,
            last_activity: row.get(5)?,
            source: row.get(6)?,
            resumed_from: row.get(7)?,
        })
    })?;
    rows.collect()
}

/// Get session history (sync version for pool.interact)
///
/// When `project_id` is provided, the query joins on the sessions table to verify
/// the session belongs to the given project (prevents cross-project data access).
pub fn get_session_history_sync(
    conn: &Connection,
    session_id: &str,
    limit: usize,
) -> rusqlite::Result<Vec<ToolHistoryEntry>> {
    get_session_history_scoped_sync(conn, session_id, None, limit)
}

/// Get session history with optional project_id scoping.
///
/// If `project_id` is `Some`, joins on sessions to ensure the session belongs
/// to the given project. If `None`, behaves like the unscoped version.
pub fn get_session_history_scoped_sync(
    conn: &Connection,
    session_id: &str,
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<ToolHistoryEntry>> {
    if let Some(pid) = project_id {
        let mut stmt = conn.prepare(
            "SELECT th.id, th.session_id, th.tool_name, th.arguments, th.result_summary,
                    th.full_result, th.success, th.created_at
             FROM tool_history th
             JOIN sessions s ON s.id = th.session_id
             WHERE th.session_id = ?1 AND s.project_id = ?2
             ORDER BY th.created_at DESC, th.id DESC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![session_id, pid, limit as i64], |row| {
            Ok(ToolHistoryEntry {
                id: row.get(0)?,
                session_id: row.get(1)?,
                tool_name: row.get(2)?,
                arguments: row.get(3)?,
                result_summary: row.get(4)?,
                full_result: row.get(5)?,
                success: row.get::<_, i32>(6)? != 0,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, tool_name, arguments, result_summary, full_result, success, created_at
             FROM tool_history
             WHERE session_id = ?
             ORDER BY created_at DESC, id DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(ToolHistoryEntry {
                id: row.get(0)?,
                session_id: row.get(1)?,
                tool_name: row.get(2)?,
                arguments: row.get(3)?,
                result_summary: row.get(4)?,
                full_result: row.get(5)?,
                success: row.get::<_, i32>(6)? != 0,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect()
    }
}

/// Get hours since last completed session for a project.
/// Returns `None` if no previous completed session exists.
pub fn get_absence_duration_sync(conn: &Connection, project_id: i64) -> Option<i64> {
    conn.query_row(
        "SELECT (strftime('%s', 'now') - strftime('%s', last_activity)) / 3600
         FROM sessions
         WHERE project_id = ? AND status = 'completed'
         ORDER BY last_activity DESC
         LIMIT 1",
        params![project_id],
        |row| row.get::<_, i64>(0),
    )
    .ok()
}

/// Get recent decisions from memory_facts.
/// Returns `(content, created_at)` tuples ordered newest first.
pub fn get_recent_decisions_sync(
    conn: &Connection,
    project_id: i64,
    limit: i64,
) -> Vec<(String, String)> {
    let mut stmt = match conn.prepare(
        "SELECT content, created_at FROM memory_facts
         WHERE project_id = ? AND fact_type = 'decision'
         ORDER BY created_at DESC
         LIMIT ?",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(params![project_id, limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// Get recently accessed file paths from session_behavior_log.
/// Extracts unique file paths from `file_access` events in the most recent sessions.
pub fn get_recent_file_activity_sync(
    conn: &Connection,
    project_id: i64,
    limit: i64,
) -> Vec<String> {
    // Get file_access events from recent sessions, extract unique file paths
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT json_extract(event_data, '$.file_path') AS fp
         FROM session_behavior_log
         WHERE project_id = ? AND event_type = 'file_access'
           AND json_extract(event_data, '$.file_path') IS NOT NULL
         ORDER BY created_at DESC
         LIMIT ?",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(params![project_id, limit], |row| row.get::<_, String>(0))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

/// Build session recap - sync version for pool.interact()
pub fn build_session_recap_sync(conn: &Connection, project_id: Option<i64>) -> String {
    use super::project::get_project_info_sync;
    use super::tasks::{get_active_goals_sync, get_pending_tasks_sync};

    let mut recap_parts = Vec::new();

    // Get project name if available
    let project_name = project_id.and_then(|pid| {
        get_project_info_sync(conn, pid)
            .ok()
            .flatten()
            .and_then(|(name, _path)| name)
    });

    // Absence-aware header
    let header = match (project_id, &project_name) {
        (Some(pid), Some(name)) => match get_absence_duration_sync(conn, pid) {
            None | Some(0) => format!("--- {} ---", name),
            Some(h) if h < 1 => format!("--- {} ---", name),
            Some(h) if h < 24 => {
                format!(
                    "--- Welcome back to {}! (last session {}h ago) ---",
                    name, h
                )
            }
            Some(h) if h < 168 => {
                let days = h / 24;
                format!(
                    "--- Welcome back to {}! (last session {} day{} ago) ---",
                    name,
                    days,
                    if days == 1 { "" } else { "s" }
                )
            }
            Some(h) => {
                let days = h / 24;
                format!(
                    "--- It's been {} days! Here's your {} context ---",
                    days, name
                )
            }
        },
        (_, Some(name)) => format!("--- {} ---", name),
        _ => "--- Session Recap ---".to_string(),
    };
    recap_parts.push(header);

    // Recent sessions (excluding current)
    if let Some(pid) = project_id
        && let Ok(sessions) = get_recent_sessions_sync(conn, pid, 2)
    {
        let recent: Vec<_> = sessions.iter().filter(|s| s.status != "active").collect();
        if !recent.is_empty() {
            let mut session_lines = Vec::new();
            for sess in recent {
                let short_id = truncate_at_boundary(&sess.id, 8);
                let timestamp = truncate_at_boundary(&sess.last_activity, 16);
                if let Some(ref summary) = sess.summary {
                    session_lines.push(format!("• [{}] {} - {}", short_id, timestamp, summary));
                } else {
                    session_lines.push(format!("• [{}] {}", short_id, timestamp));
                }
            }
            recap_parts.push(format!("Recent sessions:\n{}", session_lines.join("\n")));
        }
    }

    // Pending tasks
    if let Ok(tasks) = get_pending_tasks_sync(conn, project_id, 3)
        && !tasks.is_empty()
    {
        let task_lines: Vec<String> = tasks
            .iter()
            .map(|t| format!("• [ ] {} ({})", t.title, t.priority))
            .collect();
        recap_parts.push(format!("Pending tasks:\n{}", task_lines.join("\n")));
    }

    // Active goals
    if let Ok(goals) = get_active_goals_sync(conn, project_id, 3)
        && !goals.is_empty()
    {
        let goal_lines: Vec<String> = goals
            .iter()
            .map(|g| format!("• {} ({}%) - {}", g.title, g.progress_percent, g.status))
            .collect();
        recap_parts.push(format!("Active goals:\n{}", goal_lines.join("\n")));
    }

    // Recent decisions
    if let Some(pid) = project_id {
        let decisions = get_recent_decisions_sync(conn, pid, 5);
        if !decisions.is_empty() {
            let decision_lines: Vec<String> = decisions
                .iter()
                .map(|(content, _created_at)| format!("* {}", truncate(content, 120)))
                .collect();
            recap_parts.push(format!("Recent decisions:\n{}", decision_lines.join("\n")));
        }
    }

    // Recently active files
    if let Some(pid) = project_id {
        let files = get_recent_file_activity_sync(conn, pid, 10);
        if !files.is_empty() {
            recap_parts.push(format!("Recently active files:\n* {}", files.join(", ")));
        }
    }

    // Cross-project preferences (patterns used across multiple projects)
    if let Some(pid) = project_id
        && let Ok(prefs) = super::cross_project::get_cross_project_preferences_sync(conn, pid, 3)
        && !prefs.is_empty()
    {
        recap_parts.push(super::cross_project::format_cross_project_preferences(
            &prefs,
        ));
    }

    // Insights digest (pondering + proactive + doc gaps)
    if let Some(pid) = project_id
        && let Ok(insights) = super::insights::get_unified_insights_sync(conn, pid, None, 0.5, 7, 5)
        && !insights.is_empty()
    {
        let insight_lines: Vec<String> = insights
            .iter()
            .map(|i| {
                format!(
                    "• [{}] {} ({:.0}%)",
                    i.source,
                    i.description,
                    i.confidence * 100.0,
                )
            })
            .collect();
        recap_parts.push(format!("Insights digest:\n{}", insight_lines.join("\n")));
    }

    // Return formatted recap content
    recap_parts.join("\n\n")
}

/// Get tool call count and unique tools for a session - sync version
pub fn get_session_stats_sync(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<(usize, Vec<String>)> {
    // Get count
    let count: usize = conn.query_row(
        "SELECT COUNT(*) FROM tool_history WHERE session_id = ?",
        params![session_id],
        |row| row.get(0),
    )?;

    // Get unique tool names (top 5 most used)
    let mut stmt = conn.prepare(
        "SELECT tool_name, COUNT(*) as cnt FROM tool_history
         WHERE session_id = ?
         GROUP BY tool_name
         ORDER BY cnt DESC
         LIMIT 5",
    )?;
    let tools: Vec<String> = stmt
        .query_map(params![session_id], |row| row.get(0))?
        .filter_map(super::log_and_discard)
        .collect();

    Ok((count, tools))
}

/// Close a session by setting its status to completed and optionally adding a summary
pub fn close_session_sync(
    conn: &Connection,
    session_id: &str,
    summary: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET status = 'completed', summary = COALESCE(?2, summary), last_activity = datetime('now')
         WHERE id = ?1",
        params![session_id, summary],
    )?;
    Ok(())
}

/// Get stale active sessions (no activity for given minutes)
/// Returns (session_id, project_id, tool_count)
pub fn get_stale_sessions_sync(
    conn: &Connection,
    stale_minutes: i64,
) -> rusqlite::Result<Vec<(String, Option<i64>, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.project_id,
                (SELECT COUNT(*) FROM tool_history WHERE session_id = s.id) as tool_count
         FROM sessions s
         WHERE s.status = 'active'
           AND s.last_activity < datetime('now', '-' || ? || ' minutes')
         ORDER BY s.last_activity ASC
         LIMIT 20",
    )?;
    let rows = stmt.query_map(params![stale_minutes], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

/// Get tool history summary for a session (for LLM summarization)
pub fn get_session_tool_summary_sync(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<String> {
    let mut stmt = conn.prepare(
        "SELECT tool_name, arguments, result_summary, success
         FROM tool_history
         WHERE session_id = ?
         ORDER BY created_at ASC
         LIMIT 50",
    )?;

    let entries: Vec<String> = stmt
        .query_map(params![session_id], |row| {
            let tool: String = row.get(0)?;
            let args: Option<String> = row.get(1)?;
            let result: Option<String> = row.get(2)?;
            let success: i32 = row.get(3)?;

            let status = if success != 0 { "✓" } else { "✗" };
            let args_preview = args.map(|a| truncate(&a, 100)).unwrap_or_default();
            let result_preview = result.map(|r| truncate(&r, 150)).unwrap_or_default();

            Ok(format!(
                "{} {}({}) -> {}",
                status, tool, args_preview, result_preview
            ))
        })?
        .filter_map(super::log_and_discard)
        .collect();

    Ok(entries.join("\n"))
}

/// Get completed sessions that need summaries
/// Returns (session_id, project_id, tool_count)
/// Includes sessions with enough tool_history entries OR behavior_log events.
pub fn get_sessions_needing_summary_sync(
    conn: &Connection,
) -> rusqlite::Result<Vec<(String, Option<i64>, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.project_id,
                COALESCE(
                    NULLIF((SELECT COUNT(*) FROM tool_history WHERE session_id = s.id), 0),
                    (SELECT COUNT(*) FROM session_behavior_log WHERE session_id = s.id AND event_type = 'tool_use')
                ) as tool_count
         FROM sessions s
         WHERE s.status = 'completed'
           AND s.summary IS NULL
           AND (
               (SELECT COUNT(*) FROM tool_history WHERE session_id = s.id) >= 3
               OR (SELECT COUNT(*) FROM session_behavior_log WHERE session_id = s.id AND event_type = 'tool_use') >= 3
           )
         ORDER BY s.last_activity DESC
         LIMIT 10",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
}

/// Get session tool summary from behavior log (fallback when tool_history is empty).
/// Returns formatted text similar to get_session_tool_summary_sync.
pub fn get_session_behavior_summary_sync(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<String> {
    let mut stmt = conn.prepare(
        "SELECT event_type, event_data
         FROM session_behavior_log
         WHERE session_id = ? AND event_type IN ('tool_use', 'file_access')
         ORDER BY sequence_position ASC
         LIMIT 50",
    )?;

    let entries: Vec<String> = stmt
        .query_map(params![session_id], |row| {
            let event_type: String = row.get(0)?;
            let event_data: String = row.get(1)?;

            let data: serde_json::Value =
                serde_json::from_str(&event_data).unwrap_or(serde_json::json!({}));

            let line = match event_type.as_str() {
                "tool_use" => {
                    let tool = data
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let args = data
                        .get("args_summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    format!("✓ {}({}) -> ok", tool, truncate(args, 100))
                }
                "file_access" => {
                    let file = data.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
                    let action = data
                        .get("action")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Read");
                    format!("✓ {}({}) -> ok", action, truncate(file, 100))
                }
                _ => String::new(),
            };

            Ok(line)
        })?
        .filter_map(super::log_and_discard)
        .filter(|s| !s.is_empty())
        .collect();

    Ok(entries.join("\n"))
}

/// Update session summary (for background worker)
pub fn update_session_summary_sync(
    conn: &Connection,
    session_id: &str,
    summary: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET summary = ? WHERE id = ?",
        params![summary, session_id],
    )?;
    Ok(())
}

/// Log a tool call to history - sync version for pool.interact()
pub fn log_tool_call_sync(
    conn: &Connection,
    session_id: &str,
    tool_name: &str,
    arguments: &str,
    result_summary: &str,
    full_result: Option<&str>,
    success: bool,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO tool_history (session_id, tool_name, arguments, result_summary, full_result, success, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
        params![session_id, tool_name, arguments, result_summary, full_result, success as i32],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update session's last activity timestamp - sync version
pub fn touch_session_sync(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET last_activity = datetime('now') WHERE id = ?",
        [session_id],
    )?;
    Ok(())
}

/// Get tool history entries after a given ID - sync version
pub fn get_history_after_sync(
    conn: &Connection,
    session_id: &str,
    after_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<ToolHistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, tool_name, arguments, result_summary, full_result, success, created_at
         FROM tool_history
         WHERE session_id = ? AND id > ?
         ORDER BY id ASC
         LIMIT ?",
    )?;
    let rows = stmt.query_map(params![session_id, after_id, limit as i64], |row| {
        Ok(ToolHistoryEntry {
            id: row.get(0)?,
            session_id: row.get(1)?,
            tool_name: row.get(2)?,
            arguments: row.get(3)?,
            result_summary: row.get(4)?,
            full_result: row.get(5)?,
            success: row.get::<_, i32>(6)? != 0,
            created_at: row.get(7)?,
        })
    })?;
    rows.collect()
}

/// Row returned by session lineage query
#[derive(Debug, Clone)]
pub struct LineageRow {
    pub id: String,
    pub source: Option<String>,
    pub resumed_from: Option<String>,
    pub branch: Option<String>,
    pub started_at: String,
    pub last_activity: String,
    pub status: String,
    pub goal_count: Option<i64>,
}

/// Get session lineage for a project — sessions with resume chain info and goal counts.
/// Results ordered by last_activity DESC, grouped to aggregate goal count per session.
pub fn get_session_lineage_sync(
    conn: &Connection,
    project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<LineageRow>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.source, s.resumed_from, s.branch,
                s.started_at, s.last_activity, s.status,
                COUNT(DISTINCT sg.goal_id) AS goal_count
         FROM sessions s
         LEFT JOIN session_goals sg ON sg.session_id = s.id
         WHERE s.project_id = ?1
         GROUP BY s.id
         ORDER BY s.last_activity DESC, s.rowid DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![project_id, limit as i64], |row| {
        Ok(LineageRow {
            id: row.get(0)?,
            source: row.get(1)?,
            resumed_from: row.get(2)?,
            branch: row.get(3)?,
            started_at: row.get(4)?,
            last_activity: row.get(5)?,
            status: row.get(6)?,
            goal_count: row.get(7)?,
        })
    })?;
    rows.collect()
}
