// db/session.rs
// Session and tool history operations

use anyhow::Result;
use rusqlite::params;

use super::types::{SessionInfo, ToolHistoryEntry};
use super::Database;

impl Database {
    /// Create or update a session
    pub fn create_session(&self, session_id: &str, project_id: Option<i64>) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO sessions (id, project_id, status, started_at, last_activity)
             VALUES (?1, ?2, 'active', datetime('now'), datetime('now'))
             ON CONFLICT(id) DO UPDATE SET last_activity = datetime('now')",
            params![session_id, project_id],
        )?;
        Ok(())
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
        success: bool,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO tool_history (session_id, tool_name, arguments, result_summary, success, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![session_id, tool_name, arguments, result_summary, success as i32],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get recent tool history for a session
    pub fn get_session_history(&self, session_id: &str, limit: usize) -> Result<Vec<ToolHistoryEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, tool_name, arguments, result_summary, success, created_at
             FROM tool_history
             WHERE session_id = ?
             ORDER BY created_at DESC
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
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get tool history after a specific event ID (for sync/reconnection)
    pub fn get_history_after(&self, session_id: &str, after_id: i64, limit: usize) -> Result<Vec<ToolHistoryEntry>> {
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
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, status, summary, started_at, last_activity
             FROM sessions
             WHERE project_id = ?
             ORDER BY last_activity DESC
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
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get tool call count and unique tools for a session
    pub fn get_session_stats(&self, session_id: &str) -> Result<(usize, Vec<String>)> {
        let conn = self.conn();

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
}
