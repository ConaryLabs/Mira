// crates/mira-server/src/db/schema/memory.rs
// Documentation, users, teams, and system observations migrations
//
// Memory-specific migrations (memory_facts, vec_memory) have been removed.
// Those tables are dropped in migration v47 (drop_memory_tables).

use crate::db::migration_helpers::{column_exists, create_table_if_missing, table_exists};
use anyhow::Result;
use rusqlite::Connection;

// Note: migrate_imports_unique is now in db/schema/code.rs
// (imports table is in the separate code database)

/// Migrate to add documentation tracking tables
pub fn migrate_documentation_tables(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "documentation_tasks",
        r#"
        CREATE TABLE IF NOT EXISTS documentation_tasks (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            doc_type TEXT NOT NULL,
            doc_category TEXT NOT NULL,
            source_file_path TEXT,
            target_doc_path TEXT NOT NULL,
            priority TEXT DEFAULT 'medium',
            status TEXT DEFAULT 'pending',
            reason TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            git_commit TEXT,
            -- Safety rails for concurrent edits
            source_signature_hash TEXT,
            target_doc_checksum_at_generation TEXT,
            -- Draft content with preview for list views
            draft_content TEXT,
            draft_preview TEXT,
            draft_sha256 TEXT,
            draft_generated_at TEXT,
            reviewed_at TEXT,
            applied_at TEXT,
            -- Retry tracking
            retry_count INTEGER DEFAULT 0,
            last_error TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_doc_tasks_status ON documentation_tasks(project_id, status);
        CREATE INDEX IF NOT EXISTS idx_doc_tasks_type ON documentation_tasks(doc_type, doc_category);
        CREATE INDEX IF NOT EXISTS idx_doc_tasks_priority ON documentation_tasks(project_id, priority, status);
        -- One row per (project, target) regardless of status
        CREATE UNIQUE INDEX IF NOT EXISTS idx_doc_tasks_unique
            ON documentation_tasks(project_id, target_doc_path);

        CREATE TABLE IF NOT EXISTS documentation_inventory (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            doc_path TEXT NOT NULL,
            doc_type TEXT NOT NULL,
            doc_category TEXT,
            title TEXT,
            -- Normalized source signature hash (not raw checksum)
            source_signature_hash TEXT,
            source_symbols TEXT,
            last_seen_commit TEXT,
            is_stale INTEGER DEFAULT 0,
            staleness_reason TEXT,
            verified_at TEXT DEFAULT CURRENT_TIMESTAMP,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, doc_path)
        );
        CREATE INDEX IF NOT EXISTS idx_doc_inventory_stale ON documentation_inventory(project_id, is_stale);
    "#,
    )
}

/// Migrate documentation_inventory to add LLM-based change impact analysis columns
pub fn migrate_documentation_impact_analysis(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "documentation_inventory") {
        return Ok(());
    }

    // Add change_impact column: 'significant', 'minor', or NULL (pending analysis)
    if !column_exists(conn, "documentation_inventory", "change_impact") {
        tracing::info!("Migrating documentation_inventory to add impact analysis columns");
        conn.execute(
            "ALTER TABLE documentation_inventory ADD COLUMN change_impact TEXT",
            [],
        )?;
        conn.execute(
            "ALTER TABLE documentation_inventory ADD COLUMN change_summary TEXT",
            [],
        )?;
        conn.execute(
            "ALTER TABLE documentation_inventory ADD COLUMN impact_analyzed_at TEXT",
            [],
        )?;
    }

    Ok(())
}

/// Migrate to add users table for multi-user support
pub fn migrate_users_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "users",
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            identity TEXT UNIQUE NOT NULL,
            display_name TEXT,
            email TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_users_identity ON users(identity);
    "#,
    )
}

/// Create the system_observations table for ephemeral system-generated data.
///
/// Unlike memory_facts (removed), observations are TTL-based, not embedded,
/// and not exported. Stores health findings, scan markers, extracted outcomes, etc.
pub fn migrate_system_observations_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "system_observations",
        r#"
        CREATE TABLE IF NOT EXISTS system_observations (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            key TEXT,
            content TEXT NOT NULL,
            observation_type TEXT NOT NULL,
            category TEXT,
            confidence REAL DEFAULT 0.5,
            source TEXT NOT NULL,
            session_id TEXT,
            team_id INTEGER,
            scope TEXT DEFAULT 'project',
            expires_at TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_obs_upsert
            ON system_observations(COALESCE(project_id, -1), COALESCE(team_id, -1), scope, key)
            WHERE key IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_obs_project ON system_observations(project_id);
        CREATE INDEX IF NOT EXISTS idx_obs_type ON system_observations(project_id, observation_type);
        CREATE INDEX IF NOT EXISTS idx_obs_expires ON system_observations(expires_at) WHERE expires_at IS NOT NULL;
    "#,
    )
}

/// Migrate documentation_tasks: add skip_reason column and change 'applied' status to 'completed'
pub fn migrate_documentation_skip_reason_and_completed_status(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "documentation_tasks") {
        return Ok(());
    }

    // Add skip_reason column if missing
    if !column_exists(conn, "documentation_tasks", "skip_reason") {
        tracing::info!("Adding skip_reason column to documentation_tasks");
        conn.execute(
            "ALTER TABLE documentation_tasks ADD COLUMN skip_reason TEXT",
            [],
        )?;
    }

    // Migrate 'applied' status to 'completed'
    let updated: usize = conn.execute(
        "UPDATE documentation_tasks SET status = 'completed' WHERE status = 'applied'",
        [],
    )?;
    if updated > 0 {
        tracing::info!(
            "Migrated {} documentation_tasks from 'applied' to 'completed' status",
            updated
        );
    }

    Ok(())
}

/// Drop OLD teams tables (pre-team-intelligence-layer).
///
/// The old `teams` table lacked a `config_path` column. New tables created by
/// `db/schema/team.rs` have a different schema (teams with config_path,
/// team_sessions, team_file_ownership). We only drop the old ones.
pub fn migrate_drop_teams_tables(conn: &Connection) -> Result<()> {
    if table_exists(conn, "teams") && !column_exists(conn, "teams", "config_path") {
        conn.execute_batch(
            "DROP TABLE IF EXISTS team_members;
             DROP TABLE IF EXISTS teams;",
        )?;
    }
    Ok(())
}
