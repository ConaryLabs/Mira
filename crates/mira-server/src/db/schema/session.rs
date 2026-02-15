// crates/mira-server/src/db/schema/session.rs
// Session and chat-related migrations

use crate::db::migration_helpers::{add_column_if_missing, column_exists, table_exists};
use anyhow::Result;
use rusqlite::Connection;

/// Migrate tool_history to add full_result column for complete tool output storage
pub fn migrate_tool_history_full_result(conn: &Connection) -> Result<()> {
    // Early return if table doesn't exist
    if !table_exists(conn, "tool_history") {
        return Ok(());
    }

    // Add column if missing
    add_column_if_missing(conn, "tool_history", "full_result", "TEXT")
}

/// Migrate chat_messages to add summary_id for reversible summarization
pub fn migrate_chat_messages_summary_id(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "chat_messages") {
        return Ok(());
    }

    if !column_exists(conn, "chat_messages", "summary_id") {
        tracing::info!("Migrating chat_messages to add summary_id column");
        conn.execute(
            "ALTER TABLE chat_messages ADD COLUMN summary_id INTEGER REFERENCES chat_summaries(id) ON DELETE SET NULL",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chat_messages_summary ON chat_messages(summary_id)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate chat_summaries to add project_id column for multi-project separation
pub fn migrate_chat_summaries_project_id(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "chat_summaries") {
        return Ok(());
    }

    if !column_exists(conn, "chat_summaries", "project_id") {
        tracing::info!("Migrating chat_summaries to add project_id column");
        conn.execute(
            "ALTER TABLE chat_summaries ADD COLUMN project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chat_summaries_project ON chat_summaries(project_id, summary_level, created_at DESC)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate sessions to add branch column for branch-aware context
pub fn migrate_sessions_branch(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "sessions") {
        return Ok(());
    }

    if !column_exists(conn, "sessions", "branch") {
        tracing::info!("Adding branch column to sessions for branch-aware context");
        conn.execute("ALTER TABLE sessions ADD COLUMN branch TEXT", [])?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_branch ON sessions(branch)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate sessions to add source and resumed_from columns for session resume tracking
pub fn migrate_sessions_resume(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "sessions") {
        return Ok(());
    }

    add_column_if_missing(conn, "sessions", "source", "TEXT DEFAULT 'startup'")?;
    add_column_if_missing(conn, "sessions", "resumed_from", "TEXT")?;

    // Index for lineage queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_resumed_from ON sessions(resumed_from)",
        [],
    )?;

    Ok(())
}

/// Migration v42: Create session_goals junction table for goal-session linkage
pub fn migrate_session_goals_table(conn: &Connection) -> Result<()> {
    use crate::db::migration_helpers::create_table_if_missing;
    create_table_if_missing(
        conn,
        "session_goals",
        "CREATE TABLE IF NOT EXISTS session_goals (
            id INTEGER PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES sessions(id),
            goal_id INTEGER NOT NULL REFERENCES goals(id),
            interaction_type TEXT NOT NULL,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(session_id, goal_id, interaction_type)
        )",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_session_goals_goal ON session_goals(goal_id, created_at DESC);
         CREATE INDEX IF NOT EXISTS idx_session_goals_session ON session_goals(session_id);"
    )?;
    Ok(())
}

/// Create session_snapshots table for lightweight session state capture on stop
pub fn migrate_session_snapshots_table(conn: &Connection) -> Result<()> {
    use crate::db::migration_helpers::create_table_if_missing;
    create_table_if_missing(
        conn,
        "session_snapshots",
        r#"
        CREATE TABLE IF NOT EXISTS session_snapshots (
            id INTEGER PRIMARY KEY,
            session_id TEXT NOT NULL UNIQUE REFERENCES sessions(id),
            snapshot TEXT NOT NULL,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_session_snapshots_session ON session_snapshots(session_id);
    "#,
    )
}
