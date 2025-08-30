// src/memory/sqlite/migration.rs
//! Handles migrations for SQLite: ensures chat_history table matches latest schema.
//! Run this at startup to guarantee schema compatibility.

use anyhow::Result;
use sqlx::{Executor, SqlitePool};

/// Latest base schema for chat_history (pre-Phase 4 columns are here).
const CREATE_CHAT_HISTORY: &str = r#"
CREATE TABLE IF NOT EXISTS chat_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp DATETIME NOT NULL,
    embedding BLOB,
    salience REAL,
    tags TEXT,
    summary TEXT,
    memory_type TEXT,
    logprobs TEXT,
    moderation_flag BOOLEAN,
    system_fingerprint TEXT
);
"#;

/// Create projects table for Phase 1
const CREATE_PROJECTS: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    tags TEXT,
    owner TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
"#;

/// Create artifacts table for Phase 1
const CREATE_ARTIFACTS: &str = r#"
CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('code', 'image', 'log', 'note', 'markdown')),
    content TEXT,
    version INTEGER DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
"#;

/// Create git_repo_attachments table for git repository integration
const CREATE_GIT_REPO_ATTACHMENTS: &str = r#"
CREATE TABLE IF NOT EXISTS git_repo_attachments (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    repo_url TEXT NOT NULL,
    local_path TEXT NOT NULL,
    import_status TEXT NOT NULL,
    last_imported_at INTEGER,
    last_sync_at INTEGER,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
"#;

/// Add project_id column to chat_history to link messages to projects
const ALTER_CHAT_HISTORY_ADD_PROJECT: &str = r#"
ALTER TABLE chat_history ADD COLUMN project_id TEXT REFERENCES projects(id);
"#;

/// Phase 4: new columns
const ALTER_CHAT_HISTORY_ADD_PINNED: &str = r#"
ALTER TABLE chat_history ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
"#;

const ALTER_CHAT_HISTORY_ADD_SUBJECT_TAG: &str = r#"
ALTER TABLE chat_history ADD COLUMN subject_tag TEXT;
"#;

const ALTER_CHAT_HISTORY_ADD_LAST_ACCESSED: &str = r#"
ALTER TABLE chat_history ADD COLUMN last_accessed DATETIME;
"#;

/// Create indices for performance (Phase 1 + Phase 4)
const CREATE_INDICES: &str = r#"
-- Project-related
CREATE INDEX IF NOT EXISTS idx_artifacts_project_id ON artifacts(project_id);
CREATE INDEX IF NOT EXISTS idx_chat_history_project_id ON chat_history(project_id);
CREATE INDEX IF NOT EXISTS idx_projects_updated_at ON projects(updated_at);
CREATE INDEX IF NOT EXISTS idx_git_repo_project ON git_repo_attachments(project_id);
CREATE INDEX IF NOT EXISTS idx_git_repo_url ON git_repo_attachments(repo_url);

-- Phase 4: recall/decay helpers
CREATE INDEX IF NOT EXISTS idx_chat_history_pinned ON chat_history(pinned);
CREATE INDEX IF NOT EXISTS idx_chat_history_subject_tag ON chat_history(subject_tag);
CREATE INDEX IF NOT EXISTS idx_chat_history_last_accessed ON chat_history(last_accessed);
CREATE INDEX IF NOT EXISTS idx_chat_history_salience ON chat_history(salience);

-- Useful for recency pulls
CREATE INDEX IF NOT EXISTS idx_chat_history_timestamp ON chat_history(timestamp);
"#;

/// Cheap helper to test for a column's existence.
async fn column_exists(pool: &SqlitePool, table: &str, col: &str) -> Result<bool> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?",
    )
    .bind(table)
    .bind(col)
    .fetch_one(pool)
    .await?;
    Ok(exists > 0)
}

/// Runs all required migrations for SQLite backend. Idempotent.
pub async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    // Base tables
    pool.execute(CREATE_CHAT_HISTORY).await?;
    pool.execute(CREATE_PROJECTS).await?;
    pool.execute(CREATE_ARTIFACTS).await?;
    pool.execute(CREATE_GIT_REPO_ATTACHMENTS).await?;

    // Project link on chat_history
    if !column_exists(pool, "chat_history", "project_id").await? {
        pool.execute(ALTER_CHAT_HISTORY_ADD_PROJECT).await?;
    }

    // Phase 4 columns on chat_history (pinning/subjects/recency)
    if !column_exists(pool, "chat_history", "pinned").await? {
        pool.execute(ALTER_CHAT_HISTORY_ADD_PINNED).await?;
    }
    if !column_exists(pool, "chat_history", "subject_tag").await? {
        pool.execute(ALTER_CHAT_HISTORY_ADD_SUBJECT_TAG).await?;
    }
    if !column_exists(pool, "chat_history", "last_accessed").await? {
        pool.execute(ALTER_CHAT_HISTORY_ADD_LAST_ACCESSED).await?;
    }

    // Indices
    pool.execute(CREATE_INDICES).await?;

    Ok(())
}
