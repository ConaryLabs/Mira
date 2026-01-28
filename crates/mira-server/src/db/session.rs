// db/session.rs
// Session and tool history operations

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};

use super::Database;
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

/// Get recent sessions for a project (sync version for pool.interact)
pub fn get_recent_sessions_sync(
    conn: &Connection,
    project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<SessionInfo>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, status, summary, started_at, last_activity
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

/// Get recent pondering insights for a project
pub fn get_recent_insights_sync(
    conn: &Connection,
    project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<(String, String, f64)>> {
    let mut stmt = conn.prepare(
        r#"SELECT pattern_type, pattern_data, confidence
           FROM behavior_patterns
           WHERE project_id = ?
             AND last_triggered_at > datetime('now', '-7 days')
           ORDER BY last_triggered_at DESC
           LIMIT ?"#,
    )?;
    let rows = stmt.query_map(params![project_id, limit as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
        ))
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
    recap_parts.push(format!(
        "╔══════════════════════════════════════╗\n\
         ║   {}      ║\n\
         ╚══════════════════════════════════════╝",
        welcome
    ));

    // Time since last chat
    if let Ok(Some(last_chat_time)) = get_last_chat_time_sync(conn) {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(&last_chat_time) {
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
    }

    // Recent sessions (excluding current)
    if let Some(pid) = project_id {
        if let Ok(sessions) = get_recent_sessions_sync(conn, pid, 2) {
            let recent: Vec<_> = sessions.iter().filter(|s| s.status != "active").collect();
            if !recent.is_empty() {
                let mut session_lines = Vec::new();
                for sess in recent {
                    let short_id = &sess.id[..8.min(sess.id.len())];
                    let timestamp = &sess.last_activity[..16.min(sess.last_activity.len())];
                    if let Some(ref summary) = sess.summary {
                        session_lines.push(format!("• [{}] {} - {}", short_id, timestamp, summary));
                    } else {
                        session_lines.push(format!("• [{}] {}", short_id, timestamp));
                    }
                }
                recap_parts.push(format!("Recent sessions:\n{}", session_lines.join("\n")));
            }
        }
    }

    // Pending tasks
    if let Ok(tasks) = get_pending_tasks_sync(conn, project_id, 3) {
        if !tasks.is_empty() {
            let task_lines: Vec<String> = tasks
                .iter()
                .map(|t| format!("• [ ] {} ({})", t.title, t.priority))
                .collect();
            recap_parts.push(format!("Pending tasks:\n{}", task_lines.join("\n")));
        }
    }

    // Active goals
    if let Ok(goals) = get_active_goals_sync(conn, project_id, 3) {
        if !goals.is_empty() {
            let goal_lines: Vec<String> = goals
                .iter()
                .map(|g| format!("• {} ({}%) - {}", g.title, g.progress_percent, g.status))
                .collect();
            recap_parts.push(format!("Active goals:\n{}", goal_lines.join("\n")));
        }
    }

    // Pondering insights (from active reasoning)
    if let Some(pid) = project_id {
        if let Ok(insights) = get_recent_insights_sync(conn, pid, 3) {
            if !insights.is_empty() {
                let insight_lines: Vec<String> = insights
                    .iter()
                    .filter_map(|(pattern_type, pattern_data, confidence)| {
                        // Extract description from pattern_data JSON
                        if let Ok(data) = serde_json::from_str::<serde_json::Value>(pattern_data) {
                            if let Some(desc) = data.get("description").and_then(|d| d.as_str()) {
                                return Some(format!(
                                    "• [{}] {} ({:.0}%)",
                                    pattern_type,
                                    desc,
                                    confidence * 100.0
                                ));
                            }
                        }
                        None
                    })
                    .collect();
                if !insight_lines.is_empty() {
                    recap_parts.push(format!("Recent insights:\n{}", insight_lines.join("\n")));
                }
            }
        }
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
        .filter_map(|r| r.ok())
        .collect();

    Ok((count, tools))
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

// ============================================================================
// Database impl methods
// ============================================================================

impl Database {
    /// Create or update a session
    pub fn create_session(&self, session_id: &str, project_id: Option<i64>) -> Result<()> {
        create_session_sync(&self.conn(), session_id, project_id).map_err(Into::into)
    }

    /// Update session's last activity timestamp
    pub fn touch_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE sessions SET last_activity = datetime('now') WHERE id = ?",
            [session_id],
        )?;
        Ok(())
    }

    /// Log a tool call to history
    pub fn log_tool_call(
        &self,
        session_id: &str,
        tool_name: &str,
        arguments: &str,
        result_summary: &str,
        full_result: Option<&str>,
        success: bool,
    ) -> Result<i64> {
        log_tool_call_sync(
            &self.conn(),
            session_id,
            tool_name,
            arguments,
            result_summary,
            full_result,
            success,
        )
        .map_err(Into::into)
    }

    /// Get recent tool history for a session
    pub fn get_session_history(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ToolHistoryEntry>> {
        get_session_history_sync(&self.conn(), session_id, limit).map_err(Into::into)
    }

    /// Get tool history after a specific event ID (for sync/reconnection)
    pub fn get_history_after(
        &self,
        session_id: &str,
        after_id: i64,
        limit: usize,
    ) -> Result<Vec<ToolHistoryEntry>> {
        let conn = self.conn();
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
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get recent sessions for a project
    pub fn get_recent_sessions(&self, project_id: i64, limit: usize) -> Result<Vec<SessionInfo>> {
        get_recent_sessions_sync(&self.conn(), project_id, limit).map_err(Into::into)
    }

    /// Get tool call count and unique tools for a session
    pub fn get_session_stats(&self, session_id: &str) -> Result<(usize, Vec<String>)> {
        get_session_stats_sync(&self.conn(), session_id).map_err(Into::into)
    }

    /// Build session recap with recent activity, pending tasks, and active goals
    /// This is the single source of truth used by both MCP and chat interfaces
    pub fn build_session_recap(&self, project_id: Option<i64>) -> String {
        build_session_recap_sync(&self.conn(), project_id)
    }
}
