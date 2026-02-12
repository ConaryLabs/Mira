// db/session.rs
// Session and tool history operations

use chrono::{DateTime, Utc};
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
pub fn get_session_history_sync(
    conn: &Connection,
    session_id: &str,
    limit: usize,
) -> rusqlite::Result<Vec<ToolHistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, tool_name, arguments, result_summary, success, created_at
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
            success: row.get::<_, i32>(5)? != 0,
            created_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

/// Build session recap - sync version for pool.interact()
pub fn build_session_recap_sync(conn: &Connection, project_id: Option<i64>) -> String {
    use super::chat::get_last_chat_time_sync;
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

    // Welcome header
    let welcome = if let Some(name) = project_name {
        format!("Welcome back to {} project!", name)
    } else {
        "Welcome back!".to_string()
    };
    recap_parts.push(format!("--- {} ---", welcome));

    // Time since last chat
    if let Ok(Some(last_chat_time)) = get_last_chat_time_sync(conn)
        && let Ok(parsed) = DateTime::parse_from_rfc3339(&last_chat_time)
    {
        let now = Utc::now();
        let duration = now.signed_duration_since(parsed);
        let hours = duration.num_hours();
        let minutes = duration.num_minutes() % 60;
        let time_ago = if hours > 0 {
            format!("{} hours, {} minutes ago", hours, minutes)
        } else {
            format!("{} minutes ago", minutes)
        };
        recap_parts.push(format!("Last chat: {}", time_ago));
    }

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
pub fn get_sessions_needing_summary_sync(
    conn: &Connection,
) -> rusqlite::Result<Vec<(String, Option<i64>, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.project_id,
                (SELECT COUNT(*) FROM tool_history WHERE session_id = s.id) as tool_count
         FROM sessions s
         WHERE s.status = 'completed'
           AND s.summary IS NULL
           AND (SELECT COUNT(*) FROM tool_history WHERE session_id = s.id) >= 3
         ORDER BY s.last_activity DESC
         LIMIT 10",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
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
        "SELECT id, session_id, tool_name, arguments, result_summary, success, created_at
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
            success: row.get::<_, i32>(5)? != 0,
            created_at: row.get(6)?,
        })
    })?;
    rows.collect()
}
