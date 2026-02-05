// crates/mira-server/src/db/schema/session_tasks.rs
// Schema migration for session_tasks snapshot table

use anyhow::Result;
use rusqlite::Connection;

use crate::db::migration_helpers::create_table_if_missing;

/// Create session_tasks and session_task_iterations tables
pub fn migrate_session_tasks_tables(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "session_tasks",
        r#"
        CREATE TABLE IF NOT EXISTS session_tasks (
            id INTEGER PRIMARY KEY,
            project_id INTEGER NOT NULL,
            session_id TEXT,
            native_task_list_id TEXT,
            native_task_id TEXT,
            subject TEXT NOT NULL,
            description TEXT,
            status TEXT DEFAULT 'pending',
            raw_payload TEXT,
            iteration INTEGER DEFAULT 0,
            goal_id INTEGER,
            milestone_id INTEGER,
            version INTEGER DEFAULT 1,
            created_at TEXT DEFAULT (datetime('now')),
            updated_at TEXT DEFAULT (datetime('now')),
            completed_at TEXT
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_session_tasks_native
            ON session_tasks(native_task_list_id, native_task_id)
            WHERE native_task_list_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_session_tasks_project ON session_tasks(project_id, status);
    "#,
    )?;

    create_table_if_missing(
        conn,
        "session_task_iterations",
        r#"
        CREATE TABLE IF NOT EXISTS session_task_iterations (
            id INTEGER PRIMARY KEY,
            project_id INTEGER NOT NULL,
            session_id TEXT,
            iteration INTEGER,
            tasks_completed INTEGER DEFAULT 0,
            tasks_remaining INTEGER DEFAULT 0,
            summary TEXT,
            created_at TEXT DEFAULT (datetime('now'))
        );
    "#,
    )?;

    Ok(())
}
