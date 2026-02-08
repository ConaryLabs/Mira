// crates/mira-server/src/db/schema/reviews.rs
// Corrections, embeddings usage, diff analysis, and LLM usage tracking migrations

use crate::db::migration_helpers::{column_exists, create_table_if_missing, table_exists};
use anyhow::Result;
use rusqlite::Connection;

/// Migrate corrections table to add learning columns
pub fn migrate_corrections_learning_columns(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "corrections") {
        return Ok(());
    }

    if !column_exists(conn, "corrections", "occurrence_count") {
        tracing::info!("Adding learning columns to corrections table");
        conn.execute_batch(
            "ALTER TABLE corrections ADD COLUMN occurrence_count INTEGER DEFAULT 1;
             ALTER TABLE corrections ADD COLUMN acceptance_rate REAL DEFAULT 1.0;",
        )?;
    }

    Ok(())
}

/// Migrate to add embeddings_usage table for embedding cost tracking
pub fn migrate_embeddings_usage_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "embeddings_usage",
        r#"
        CREATE TABLE IF NOT EXISTS embeddings_usage (
            id INTEGER PRIMARY KEY,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            tokens INTEGER NOT NULL,
            text_count INTEGER NOT NULL,
            cost_estimate REAL,
            project_id INTEGER REFERENCES projects(id),
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_embeddings_usage_provider ON embeddings_usage(provider, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_embeddings_usage_project ON embeddings_usage(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_embeddings_usage_created ON embeddings_usage(created_at DESC);
    "#,
    )
}

/// Migrate to add diff_analyses table for semantic diff analysis
pub fn migrate_diff_analyses_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "diff_analyses",
        r#"
        CREATE TABLE IF NOT EXISTS diff_analyses (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            from_commit TEXT NOT NULL,
            to_commit TEXT NOT NULL,
            analysis_type TEXT DEFAULT 'commit',
            changes_json TEXT,
            impact_json TEXT,
            risk_json TEXT,
            summary TEXT,
            files_changed INTEGER,
            lines_added INTEGER,
            lines_removed INTEGER,
            status TEXT DEFAULT 'complete',
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_diff_commits ON diff_analyses(project_id, from_commit, to_commit);
        CREATE INDEX IF NOT EXISTS idx_diff_created ON diff_analyses(project_id, created_at DESC);
    "#,
    )
}

/// Migrate to add files_json column to diff_analyses
pub fn migrate_diff_analyses_files_json(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "diff_analyses", "files_json") {
        tracing::info!("Adding files_json column to diff_analyses table");
        conn.execute_batch("ALTER TABLE diff_analyses ADD COLUMN files_json TEXT;")?;
    }
    Ok(())
}

/// Migrate to add diff_outcomes table for tracking change outcomes
pub fn migrate_diff_outcomes_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "diff_outcomes",
        r#"
        CREATE TABLE IF NOT EXISTS diff_outcomes (
            id INTEGER PRIMARY KEY,
            diff_analysis_id INTEGER NOT NULL REFERENCES diff_analyses(id),
            project_id INTEGER REFERENCES projects(id),
            outcome_type TEXT NOT NULL,
            evidence_commit TEXT,
            evidence_message TEXT,
            time_to_outcome_seconds INTEGER,
            detected_by TEXT DEFAULT 'git_scan',
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(diff_analysis_id, outcome_type, evidence_commit)
        );
        CREATE INDEX IF NOT EXISTS idx_diff_outcomes_analysis ON diff_outcomes(diff_analysis_id);
        CREATE INDEX IF NOT EXISTS idx_diff_outcomes_project ON diff_outcomes(project_id, outcome_type);
        CREATE INDEX IF NOT EXISTS idx_diff_outcomes_type ON diff_outcomes(outcome_type);
    "#,
    )
}

/// Migrate to add llm_usage table for LLM cost/token tracking
pub fn migrate_llm_usage_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "llm_usage",
        r#"
        CREATE TABLE IF NOT EXISTS llm_usage (
            id INTEGER PRIMARY KEY,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            role TEXT NOT NULL,
            prompt_tokens INTEGER NOT NULL,
            completion_tokens INTEGER NOT NULL,
            total_tokens INTEGER NOT NULL,
            cache_hit_tokens INTEGER DEFAULT 0,
            cache_miss_tokens INTEGER DEFAULT 0,
            cost_estimate REAL,
            duration_ms INTEGER,
            project_id INTEGER REFERENCES projects(id),
            session_id TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_llm_usage_provider ON llm_usage(provider, model, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_llm_usage_role ON llm_usage(role, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_llm_usage_project ON llm_usage(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_llm_usage_session ON llm_usage(session_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_llm_usage_created ON llm_usage(created_at DESC);
    "#,
    )
}
