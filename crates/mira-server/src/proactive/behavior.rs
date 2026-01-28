// crates/mira-server/src/proactive/behavior.rs
// Behavior tracking - logs events and builds session history

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::EventType;

/// A single behavior event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorEvent {
    pub event_type: EventType,
    pub data: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Session-level behavior tracker
pub struct BehaviorTracker {
    session_id: String,
    project_id: i64,
    sequence_position: i32,
    last_event_time: Option<Instant>,
}

impl BehaviorTracker {
    pub fn new(session_id: String, project_id: i64) -> Self {
        Self {
            session_id,
            project_id,
            sequence_position: 0,
            last_event_time: None,
        }
    }

    /// Log a behavior event
    pub fn log_event(
        &mut self,
        conn: &Connection,
        event_type: EventType,
        data: serde_json::Value,
    ) -> Result<i64> {
        let time_since_last = self.last_event_time.map(|t| t.elapsed().as_millis() as i64);

        self.sequence_position += 1;
        self.last_event_time = Some(Instant::now());

        let sql = r#"
            INSERT INTO session_behavior_log
            (project_id, session_id, event_type, event_data, sequence_position, time_since_last_event_ms)
            VALUES (?, ?, ?, ?, ?, ?)
        "#;

        conn.execute(
            sql,
            rusqlite::params![
                self.project_id,
                &self.session_id,
                event_type.as_str(),
                data.to_string(),
                self.sequence_position,
                time_since_last,
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Log file access
    pub fn log_file_access(
        &mut self,
        conn: &Connection,
        file_path: &str,
        action: &str,
    ) -> Result<i64> {
        let data = serde_json::json!({
            "file_path": file_path,
            "action": action,
        });
        self.log_event(conn, EventType::FileAccess, data)
    }

    /// Log tool usage
    pub fn log_tool_use(
        &mut self,
        conn: &Connection,
        tool_name: &str,
        args_summary: Option<&str>,
    ) -> Result<i64> {
        let data = serde_json::json!({
            "tool_name": tool_name,
            "args_summary": args_summary,
        });
        self.log_event(conn, EventType::ToolUse, data)
    }

    /// Log user query/prompt
    pub fn log_query(
        &mut self,
        conn: &Connection,
        query_text: &str,
        query_type: &str,
    ) -> Result<i64> {
        // Hash or truncate the query for privacy
        let query_summary = if query_text.len() > 200 {
            format!("{}...", &query_text[..200])
        } else {
            query_text.to_string()
        };

        let data = serde_json::json!({
            "query_summary": query_summary,
            "query_type": query_type,
            "query_length": query_text.len(),
        });
        self.log_event(conn, EventType::Query, data)
    }

    /// Log context switch (e.g., moving to a different area of the codebase)
    pub fn log_context_switch(
        &mut self,
        conn: &Connection,
        from_context: &str,
        to_context: &str,
    ) -> Result<i64> {
        let data = serde_json::json!({
            "from": from_context,
            "to": to_context,
        });
        self.log_event(conn, EventType::ContextSwitch, data)
    }
}

/// Get recent events for a session
pub fn get_session_events(
    conn: &Connection,
    session_id: &str,
    limit: i64,
) -> Result<Vec<BehaviorEvent>> {
    let sql = r#"
        SELECT event_type, event_data, created_at
        FROM session_behavior_log
        WHERE session_id = ?
        ORDER BY sequence_position DESC
        LIMIT ?
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([session_id, &limit.to_string()], |row| {
        let event_type_str: String = row.get(0)?;
        let event_data_str: String = row.get(1)?;
        let created_at_str: String = row.get(2)?;

        Ok((event_type_str, event_data_str, created_at_str))
    })?;

    let mut events = Vec::new();
    for row in rows.flatten() {
        let (event_type_str, event_data_str, created_at_str) = row;

        if let Some(event_type) = EventType::from_str(&event_type_str) {
            let data: serde_json::Value = serde_json::from_str(&event_data_str).unwrap_or_default();
            let timestamp = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            events.push(BehaviorEvent {
                event_type,
                data,
                timestamp,
            });
        }
    }

    // Reverse to get chronological order
    events.reverse();
    Ok(events)
}

/// Get recent file access sequence for a project
pub fn get_recent_file_sequence(
    conn: &Connection,
    project_id: i64,
    limit: i64,
) -> Result<Vec<String>> {
    let sql = r#"
        SELECT DISTINCT json_extract(event_data, '$.file_path') as file_path
        FROM session_behavior_log
        WHERE project_id = ?
          AND event_type = 'file_access'
          AND json_extract(event_data, '$.file_path') IS NOT NULL
        ORDER BY created_at DESC
        LIMIT ?
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([project_id, limit], |row| row.get::<_, String>(0))?;

    let files: Vec<String> = rows.flatten().collect();
    Ok(files)
}

/// Get tool usage frequency for a project
pub fn get_tool_usage_stats(conn: &Connection, project_id: i64) -> Result<Vec<(String, i64)>> {
    let sql = r#"
        SELECT json_extract(event_data, '$.tool_name') as tool_name,
               COUNT(*) as usage_count
        FROM session_behavior_log
        WHERE project_id = ?
          AND event_type = 'tool_use'
          AND json_extract(event_data, '$.tool_name') IS NOT NULL
        GROUP BY tool_name
        ORDER BY usage_count DESC
        LIMIT 20
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([project_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;

    let stats: Vec<(String, i64)> = rows.flatten().collect();
    Ok(stats)
}

/// Clean up old behavior logs (keep last N days)
pub fn cleanup_old_logs(conn: &Connection, days_to_keep: i64) -> Result<usize> {
    let sql = r#"
        DELETE FROM session_behavior_log
        WHERE created_at < datetime('now', ? || ' days')
    "#;

    let deleted = conn.execute(sql, [format!("-{}", days_to_keep)])?;
    Ok(deleted)
}
