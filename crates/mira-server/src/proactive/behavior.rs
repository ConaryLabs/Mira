// crates/mira-server/src/proactive/behavior.rs
// Behavior tracking - logs events and builds session history

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::utils::truncate;

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
    /// Create a new tracker - use `for_session` if resuming an existing session
    pub fn new(session_id: String, project_id: i64) -> Self {
        Self {
            session_id,
            project_id,
            sequence_position: 0,
            last_event_time: None,
        }
    }

    /// Create a tracker for an existing session, loading the current sequence position
    /// from the database to ensure proper incrementing
    pub fn for_session(conn: &Connection, session_id: String, project_id: i64) -> Self {
        let current_position = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence_position), 0) FROM session_behavior_log WHERE session_id = ?",
                [&session_id],
                |row| row.get::<_, i32>(0),
            )
            .unwrap_or(0);

        Self {
            session_id,
            project_id,
            sequence_position: current_position,
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
        let query_summary = truncate(query_text, 200);

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
    for row in rows.filter_map(crate::db::log_and_discard) {
        let (event_type_str, event_data_str, created_at_str) = row;

        if let Ok(event_type) = event_type_str.parse::<EventType>() {
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

    let files: Vec<String> = rows.filter_map(crate::db::log_and_discard).collect();
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

    let stats: Vec<(String, i64)> = rows.filter_map(crate::db::log_and_discard).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE session_behavior_log (
                id INTEGER PRIMARY KEY,
                project_id INTEGER,
                session_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                event_data TEXT NOT NULL,
                sequence_position INTEGER,
                time_since_last_event_ms INTEGER,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_new_tracker_starts_at_zero() {
        let tracker = BehaviorTracker::new("test-session".to_string(), 1);
        assert_eq!(tracker.sequence_position, 0);
    }

    #[test]
    fn test_for_session_loads_max_position() {
        let conn = setup_test_db();

        // Insert some events with sequence positions
        conn.execute(
            "INSERT INTO session_behavior_log (session_id, project_id, event_type, event_data, sequence_position) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params!["test-session", 1, "tool_use", "{}", 1],
        ).unwrap();
        conn.execute(
            "INSERT INTO session_behavior_log (session_id, project_id, event_type, event_data, sequence_position) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params!["test-session", 1, "tool_use", "{}", 2],
        ).unwrap();
        conn.execute(
            "INSERT INTO session_behavior_log (session_id, project_id, event_type, event_data, sequence_position) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params!["test-session", 1, "file_access", "{}", 3],
        ).unwrap();

        // Create tracker for existing session
        let tracker = BehaviorTracker::for_session(&conn, "test-session".to_string(), 1);
        assert_eq!(tracker.sequence_position, 3);
    }

    #[test]
    fn test_for_session_empty_session_starts_at_zero() {
        let conn = setup_test_db();

        // Create tracker for non-existent session
        let tracker = BehaviorTracker::for_session(&conn, "new-session".to_string(), 1);
        assert_eq!(tracker.sequence_position, 0);
    }

    #[test]
    fn test_log_event_increments_position() {
        let conn = setup_test_db();

        let mut tracker = BehaviorTracker::new("test-session".to_string(), 1);
        assert_eq!(tracker.sequence_position, 0);

        tracker
            .log_event(
                &conn,
                EventType::ToolUse,
                serde_json::json!({"tool": "test"}),
            )
            .unwrap();
        assert_eq!(tracker.sequence_position, 1);

        tracker
            .log_event(
                &conn,
                EventType::FileAccess,
                serde_json::json!({"file": "test.rs"}),
            )
            .unwrap();
        assert_eq!(tracker.sequence_position, 2);

        // Verify data in database
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_behavior_log WHERE session_id = ?",
                ["test-session"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_resumed_tracker_continues_sequence() {
        let conn = setup_test_db();

        // First tracker logs 3 events
        {
            let mut tracker = BehaviorTracker::new("test-session".to_string(), 1);
            tracker.log_tool_use(&conn, "Read", None).unwrap();
            tracker.log_tool_use(&conn, "Edit", None).unwrap();
            tracker
                .log_file_access(&conn, "/path/to/file.rs", "Edit")
                .unwrap();
            assert_eq!(tracker.sequence_position, 3);
        }

        // Second tracker resumes and continues from position 3
        {
            let mut tracker = BehaviorTracker::for_session(&conn, "test-session".to_string(), 1);
            assert_eq!(tracker.sequence_position, 3);

            tracker.log_tool_use(&conn, "Write", None).unwrap();
            assert_eq!(tracker.sequence_position, 4);
        }

        // Verify sequence positions in database
        let positions: Vec<i32> = {
            let mut stmt = conn
                .prepare("SELECT sequence_position FROM session_behavior_log ORDER BY id")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .flatten()
                .collect()
        };
        assert_eq!(positions, vec![1, 2, 3, 4]);
    }
}
