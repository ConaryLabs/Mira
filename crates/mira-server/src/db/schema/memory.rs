// crates/mira-server/src/db/schema/memory.rs
// Memory, users, and teams migrations

use crate::db::migration_helpers::{column_exists, create_table_if_missing, table_exists};
use anyhow::Result;
use rusqlite::Connection;

/// Migrate memory_facts to add has_embedding column for tracking embedding status
pub fn migrate_memory_facts_has_embedding(conn: &Connection) -> Result<()> {
    // Early return if table doesn't exist
    if !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    // Add column if missing (also handles backfill)
    if !column_exists(conn, "memory_facts", "has_embedding") {
        tracing::info!("Migrating memory_facts to add has_embedding column");
        conn.execute(
            "ALTER TABLE memory_facts ADD COLUMN has_embedding INTEGER DEFAULT 0",
            [],
        )?;
        // Backfill: mark existing facts that have embeddings
        conn.execute(
            "UPDATE memory_facts SET has_embedding = 1 WHERE id IN (SELECT fact_id FROM vec_memory)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate memory_facts to add evidence-based tracking columns
pub fn migrate_memory_facts_evidence_tracking(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    if !column_exists(conn, "memory_facts", "session_count") {
        tracing::info!("Migrating memory_facts to add evidence-based tracking columns");
        conn.execute_batch(
            "ALTER TABLE memory_facts ADD COLUMN session_count INTEGER DEFAULT 1;
             ALTER TABLE memory_facts ADD COLUMN first_session_id TEXT;
             ALTER TABLE memory_facts ADD COLUMN last_session_id TEXT;
             ALTER TABLE memory_facts ADD COLUMN status TEXT DEFAULT 'candidate';",
        )?;

        conn.execute(
            "UPDATE memory_facts SET status = 'confirmed' WHERE confidence >= 0.8",
            [],
        )?;
    }

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memory_status ON memory_facts(status)",
        [],
    )?;

    Ok(())
}

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
        -- Uniqueness constraint to prevent duplicate tasks for same target
        CREATE UNIQUE INDEX IF NOT EXISTS idx_doc_tasks_unique
            ON documentation_tasks(project_id, target_doc_path, doc_type, doc_category)
            WHERE status = 'pending';

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

/// Migrate memory_facts to add user_id and scope columns for multi-user sharing
pub fn migrate_memory_user_scope(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    if !column_exists(conn, "memory_facts", "user_id") {
        tracing::info!("Adding user_id column to memory_facts for multi-user support");
        conn.execute("ALTER TABLE memory_facts ADD COLUMN user_id TEXT", [])?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_user ON memory_facts(user_id)",
            [],
        )?;
    }

    if !column_exists(conn, "memory_facts", "scope") {
        tracing::info!("Adding scope column to memory_facts for visibility control");
        conn.execute(
            "ALTER TABLE memory_facts ADD COLUMN scope TEXT DEFAULT 'project'",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_scope ON memory_facts(scope)",
            [],
        )?;
    }

    if !column_exists(conn, "memory_facts", "team_id") {
        tracing::info!("Adding team_id column to memory_facts for team sharing");
        conn.execute("ALTER TABLE memory_facts ADD COLUMN team_id INTEGER", [])?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_team ON memory_facts(team_id)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate memory_facts to add branch column for branch-aware context
pub fn migrate_memory_facts_branch(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    if !column_exists(conn, "memory_facts", "branch") {
        tracing::info!("Adding branch column to memory_facts for branch-aware memory");
        conn.execute("ALTER TABLE memory_facts ADD COLUMN branch TEXT", [])?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_branch ON memory_facts(branch)",
            [],
        )?;
    }

    Ok(())
}

/// Remove orphaned capability data left behind after check_capability tool removal
pub fn migrate_remove_capability_data(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    // Delete capability facts generated by the background scanner
    let deleted: usize = conn.execute(
        "DELETE FROM memory_facts WHERE fact_type = 'capability' AND category = 'codebase'",
        [],
    )?;

    // Delete capabilities scan time markers
    let markers: usize = conn.execute(
        "DELETE FROM memory_facts WHERE key = 'capabilities_scan_time'",
        [],
    )?;

    // Clean up orphaned embeddings
    if deleted > 0 {
        conn.execute(
            "DELETE FROM vec_memory WHERE fact_id NOT IN (SELECT id FROM memory_facts)",
            [],
        )?;
    }

    if deleted > 0 || markers > 0 {
        tracing::info!(
            "Cleaned up capability data: {} facts, {} scan markers removed",
            deleted,
            markers
        );
    }

    Ok(())
}

/// Migrate to add teams tables for team-based memory sharing
pub fn migrate_teams_tables(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "teams",
        r#"
        CREATE TABLE IF NOT EXISTS teams (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            created_by TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_teams_name ON teams(name);

        CREATE TABLE IF NOT EXISTS team_members (
            id INTEGER PRIMARY KEY,
            team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
            user_identity TEXT NOT NULL,
            role TEXT DEFAULT 'member',
            joined_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(team_id, user_identity)
        );
        CREATE INDEX IF NOT EXISTS idx_team_members_team ON team_members(team_id);
        CREATE INDEX IF NOT EXISTS idx_team_members_user ON team_members(user_identity);
    "#,
    )
}
