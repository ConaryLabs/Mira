// crates/mira-server/src/db/schema/session.rs
// Session and chat-related migrations

use anyhow::Result;
use rusqlite::Connection;
use crate::db::migration_helpers::{table_exists, column_exists, add_column_if_missing};

/// Migrate tool_history to add full_result column for complete tool output storage
pub fn migrate_tool_history_full_result(conn: &Connection) -> Result<()> {
    // Early return if table doesn't exist
    if !table_exists(conn, "tool_history") {
        return Ok(());
    }

    // Add column if missing
    add_column_if_missing(
        conn,
        "tool_history",
        "full_result",
        "TEXT"
    )
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
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN branch TEXT",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_branch ON sessions(branch)",
            [],
        )?;
    }

    Ok(())
}
