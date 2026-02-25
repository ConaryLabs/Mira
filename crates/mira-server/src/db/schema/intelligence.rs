// crates/mira-server/src/db/schema/intelligence.rs
// Proactive intelligence and cross-project migrations (now no-ops)

use anyhow::Result;
use rusqlite::Connection;

use crate::db::migration_helpers::create_table_if_missing;

/// Migrate to add proactive intelligence tables for behavior tracking and predictions.
///
/// Proactive prediction system removed. Tables dropped in migration v48.
/// session_behavior_log and behavior_patterns are kept (used by insights,
/// working context, error tracking, and change pattern mining).
pub fn migrate_proactive_intelligence_tables(conn: &Connection) -> Result<()> {
    // session_behavior_log is still used by multiple systems -- keep it.
    create_table_if_missing(
        conn,
        "session_behavior_log",
        r#"
        CREATE TABLE IF NOT EXISTS session_behavior_log (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            session_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            event_data TEXT NOT NULL,
            sequence_position INTEGER,
            time_since_last_event_ms INTEGER,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_behavior_log_session ON session_behavior_log(session_id, sequence_position);
        CREATE INDEX IF NOT EXISTS idx_behavior_log_project ON session_behavior_log(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_behavior_log_type ON session_behavior_log(event_type, created_at DESC);
    "#,
    )?;

    // behavior_patterns is still used by insights, pondering, change pattern mining.
    create_table_if_missing(
        conn,
        "behavior_patterns",
        r#"
        CREATE TABLE IF NOT EXISTS behavior_patterns (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            pattern_type TEXT NOT NULL,
            pattern_key TEXT NOT NULL,
            pattern_data TEXT NOT NULL,
            confidence REAL DEFAULT 0.5,
            occurrence_count INTEGER DEFAULT 1,
            last_triggered_at TEXT,
            first_seen_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, pattern_type, pattern_key)
        );
        CREATE INDEX IF NOT EXISTS idx_behavior_patterns_project ON behavior_patterns(project_id, pattern_type);
        CREATE INDEX IF NOT EXISTS idx_behavior_patterns_confidence ON behavior_patterns(confidence DESC);
        CREATE INDEX IF NOT EXISTS idx_behavior_patterns_recent ON behavior_patterns(last_triggered_at DESC);
    "#,
    )?;

    // shown_count and dismissed columns on behavior_patterns (used by insights)
    if !crate::db::migration_helpers::column_exists(conn, "behavior_patterns", "shown_count") {
        conn.execute_batch(
            "ALTER TABLE behavior_patterns ADD COLUMN shown_count INTEGER DEFAULT 0;",
        )?;
    }
    if !crate::db::migration_helpers::column_exists(conn, "behavior_patterns", "dismissed") {
        conn.execute_batch(
            "ALTER TABLE behavior_patterns ADD COLUMN dismissed INTEGER DEFAULT 0;",
        )?;
    }

    // proactive_interventions and proactive_suggestions tables dropped in v48.
    // No longer created here.

    Ok(())
}

/// Migrate to add error_patterns table for cross-session error learning
pub fn migrate_error_patterns_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "error_patterns",
        r#"
        CREATE TABLE IF NOT EXISTS error_patterns (
            id INTEGER PRIMARY KEY,
            project_id INTEGER NOT NULL REFERENCES projects(id),
            tool_name TEXT NOT NULL,
            error_fingerprint TEXT NOT NULL,
            error_template TEXT NOT NULL,
            raw_error_sample TEXT,
            fix_description TEXT,
            fix_session_id TEXT,
            occurrence_count INTEGER DEFAULT 1,
            first_seen_session_id TEXT,
            last_seen_session_id TEXT,
            resolved_at TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, tool_name, error_fingerprint)
        );
        CREATE INDEX IF NOT EXISTS idx_error_patterns_lookup
            ON error_patterns(project_id, tool_name, error_fingerprint);
        CREATE INDEX IF NOT EXISTS idx_error_patterns_unresolved
            ON error_patterns(project_id, resolved_at) WHERE resolved_at IS NULL;
    "#,
    )?;
    Ok(())
}

/// Migrate to add cross-project intelligence tables.
///
/// Tables dropped in migration v35. This is a no-op.
pub fn migrate_cross_project_intelligence_tables(_conn: &Connection) -> Result<()> {
    // Tables dropped in v35 (drop_dead_tables)
    Ok(())
}
