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

/// Create the system_observations table for ephemeral system-generated data.
///
/// Unlike memory_facts (user memories), observations are TTL-based, not embedded,
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

/// Migrate system data from memory_facts to system_observations.
///
/// Moves health findings, system markers, session events, extracted outcomes,
/// convergence alerts, and distilled data. Deduplicates keyed rows first, then
/// moves data atomically: insert -> delete -> clean orphaned embeddings.
pub fn migrate_memory_facts_to_observations(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "system_observations") || !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    // Check if there's anything to migrate
    let system_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory_facts
         WHERE fact_type IN ('health','system','session_event','extracted','tool_outcome','convergence_alert','distilled')
            OR (fact_type = 'context' AND category = 'subagent_discovery')
            OR (fact_type = 'context' AND category = 'system')",
        [],
        |row| row.get(0),
    )?;

    if system_count == 0 {
        tracing::info!("No system data to migrate from memory_facts");
        return Ok(());
    }

    tracing::info!(
        "Migrating {} system rows from memory_facts to system_observations",
        system_count
    );

    // Step 0: Deduplicate keyed rows in memory_facts.
    // memory_facts has no UNIQUE(project_id, key) constraint, and INSERT OR REPLACE
    // in insert_system_marker_sync silently creates duplicates. Keep only the latest
    // row per (project_id, key) group for system rows.
    let deduped: usize = conn.execute(
        "DELETE FROM memory_facts WHERE id NOT IN (
            SELECT MAX(id) FROM memory_facts
            WHERE key IS NOT NULL
              AND fact_type IN ('health','system','session_event','extracted','tool_outcome','convergence_alert','distilled')
            GROUP BY COALESCE(project_id, -1), key
        ) AND key IS NOT NULL
          AND fact_type IN ('health','system','session_event','extracted','tool_outcome','convergence_alert','distilled')",
        [],
    )?;
    if deduped > 0 {
        tracing::info!("Deduplicated {} keyed system rows in memory_facts", deduped);
    }

    // Step 1: INSERT into system_observations with type mapping.
    // Map fact_type -> observation_type, infer source from (fact_type, category).
    conn.execute_batch(
        "INSERT INTO system_observations
            (project_id, key, content, observation_type, category, confidence,
             source, session_id, team_id, scope, created_at, updated_at)
         SELECT
            project_id, key, content,
            -- observation_type mapping
            CASE
                WHEN fact_type = 'health' THEN 'health'
                WHEN fact_type = 'system' THEN 'system'
                WHEN fact_type = 'session_event' THEN 'session_event'
                WHEN fact_type = 'extracted' THEN 'extracted'
                WHEN fact_type = 'tool_outcome' THEN 'tool_outcome'
                WHEN fact_type = 'convergence_alert' THEN 'convergence_alert'
                WHEN fact_type = 'distilled' THEN 'distilled'
                WHEN fact_type = 'context' AND category = 'subagent_discovery' THEN 'subagent_discovery'
                WHEN fact_type = 'context' AND category = 'system' THEN 'system'
                ELSE fact_type
            END,
            category, confidence,
            -- source inference
            CASE
                WHEN fact_type = 'health' AND category IN ('todo','unimplemented','unwrap','error_handling') THEN 'code_health'
                WHEN fact_type = 'health' AND category = 'warning' THEN 'code_health'
                WHEN fact_type = 'health' AND category IN ('complexity','error_quality') THEN 'code_health'
                WHEN fact_type = 'health' AND category = 'circular_dependency' THEN 'code_health'
                WHEN fact_type = 'health' AND category IN ('architecture','unused') THEN 'code_health'
                WHEN fact_type = 'health' THEN 'code_health'
                WHEN fact_type = 'system' AND category = 'health' THEN 'code_health'
                WHEN fact_type = 'system' AND category = 'documentation' THEN 'documentation'
                WHEN fact_type = 'system' THEN 'system'
                WHEN fact_type = 'session_event' THEN 'precompact'
                WHEN fact_type = 'extracted' THEN 'precompact'
                WHEN fact_type = 'tool_outcome' THEN 'extraction'
                WHEN fact_type = 'convergence_alert' THEN 'team_monitor'
                WHEN fact_type = 'distilled' THEN 'distillation'
                WHEN fact_type = 'context' AND category = 'subagent_discovery' THEN 'subagent'
                WHEN fact_type = 'context' AND category = 'system' THEN 'project'
                ELSE 'unknown'
            END,
            first_session_id,
            team_id,
            COALESCE(scope, 'project'),
            created_at,
            COALESCE(updated_at, created_at)
         FROM memory_facts
         WHERE fact_type IN ('health','system','session_event','extracted','tool_outcome','convergence_alert','distilled')
            OR (fact_type = 'context' AND category = 'subagent_discovery')
            OR (fact_type = 'context' AND category = 'system')",
    )?;

    // Step 2: DELETE migrated rows from memory_facts
    let deleted: usize = conn.execute(
        "DELETE FROM memory_facts
         WHERE fact_type IN ('health','system','session_event','extracted','tool_outcome','convergence_alert','distilled')
            OR (fact_type = 'context' AND category = 'subagent_discovery')
            OR (fact_type = 'context' AND category = 'system')",
        [],
    )?;

    // Step 3: Clean orphaned vec_memory entries
    let orphaned: usize = conn.execute(
        "DELETE FROM vec_memory WHERE fact_id NOT IN (SELECT id FROM memory_facts)",
        [],
    )?;

    tracing::info!(
        "Migration complete: {} rows moved to system_observations, {} orphaned embeddings cleaned",
        deleted,
        orphaned
    );

    Ok(())
}

/// Migrate memory_facts to add suspicious column for prompt injection detection
pub fn migrate_memory_facts_suspicious(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    if !column_exists(conn, "memory_facts", "suspicious") {
        tracing::info!("Adding suspicious column to memory_facts for injection detection");
        conn.execute(
            "ALTER TABLE memory_facts ADD COLUMN suspicious INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
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
