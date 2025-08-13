// src/memory/sqlite/migration.rs
//! Handles migrations for SQLite: ensures chat_history table matches latest schema.
//! Run this at startup to guarantee schema compatibility.
use sqlx::{SqlitePool, Executor};
use anyhow::Result;

/// Latest schema for chat_history. Add columns here as you evolve fields.
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

/// Create index for faster project queries
const CREATE_PROJECT_INDICES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_artifacts_project_id ON artifacts(project_id);
CREATE INDEX IF NOT EXISTS idx_chat_history_project_id ON chat_history(project_id);
CREATE INDEX IF NOT EXISTS idx_projects_updated_at ON projects(updated_at);
CREATE INDEX IF NOT EXISTS idx_git_repo_project ON git_repo_attachments(project_id);
CREATE INDEX IF NOT EXISTS idx_git_repo_url ON git_repo_attachments(repo_url);
"#;

/// Runs all required migrations for SQLite backend.
/// Safe to call at every startup (idempotent).
pub async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    // Original chat history table
    pool.execute(CREATE_CHAT_HISTORY).await?;
    
    // Phase 1: Project system tables
    pool.execute(CREATE_PROJECTS).await?;
    pool.execute(CREATE_ARTIFACTS).await?;
    
    // Git repository attachments table
    pool.execute(CREATE_GIT_REPO_ATTACHMENTS).await?;
    
    // Check if project_id column already exists before trying to add it
    let has_project_id: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('chat_history') WHERE name = 'project_id'"
    )
    .fetch_one(pool)
    .await?;
    
    if !has_project_id {
        pool.execute(ALTER_CHAT_HISTORY_ADD_PROJECT).await?;
    }
    
    // Create indices for performance
    pool.execute(CREATE_PROJECT_INDICES).await?;
    
    Ok(())
}
