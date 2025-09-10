// src/memory/sqlite/migration.rs
//! Handles migrations for SQLite: ensures chat_history table matches latest schema.
//! Run this at startup to guarantee schema compatibility.
//! Sprint 3: Added session_message_counts table for rolling summaries

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

/// Sprint 3: Create session_message_counts table for rolling summaries
const CREATE_SESSION_MESSAGE_COUNTS: &str = r#"
CREATE TABLE IF NOT EXISTS session_message_counts (
    session_id TEXT PRIMARY KEY,
    count INTEGER NOT NULL DEFAULT 0,
    last_summary_10_at INTEGER,
    last_summary_100_at INTEGER,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
"#;

/// Sprint 3: Create summary_metadata table to track summary generations
const CREATE_SUMMARY_METADATA: &str = r#"
CREATE TABLE IF NOT EXISTS summary_metadata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    summary_type TEXT NOT NULL CHECK (summary_type IN ('rolling_10', 'rolling_100', 'snapshot')),
    message_count_at_creation INTEGER NOT NULL,
    summary_entry_id INTEGER,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (summary_entry_id) REFERENCES chat_history(id) ON DELETE CASCADE
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

/// Create indices for performance (Phase 1 + Phase 4 + Sprint 3)
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
CREATE INDEX IF NOT EXISTS idx_chat_history_session_id ON chat_history(session_id);

-- Sprint 3: Rolling summaries support
CREATE INDEX IF NOT EXISTS idx_session_message_counts_updated ON session_message_counts(updated_at);
CREATE INDEX IF NOT EXISTS idx_summary_metadata_session ON summary_metadata(session_id);
CREATE INDEX IF NOT EXISTS idx_summary_metadata_type ON summary_metadata(summary_type);

-- For finding summaries in chat_history
CREATE INDEX IF NOT EXISTS idx_chat_history_tags ON chat_history(tags);
CREATE INDEX IF NOT EXISTS idx_chat_history_memory_type ON chat_history(memory_type);
"#;

/// Sprint 3: Initialize message counts from existing data
const INITIALIZE_MESSAGE_COUNTS: &str = r#"
INSERT OR IGNORE INTO session_message_counts (session_id, count, updated_at)
SELECT 
    session_id,
    COUNT(*) as count,
    CURRENT_TIMESTAMP
FROM chat_history
WHERE role IN ('user', 'assistant')
GROUP BY session_id;
"#;

/// Sprint 3: Trigger to auto-update message counts on new messages
const CREATE_MESSAGE_COUNT_TRIGGER: &str = r#"
CREATE TRIGGER IF NOT EXISTS update_message_count_on_insert
AFTER INSERT ON chat_history
WHEN NEW.role IN ('user', 'assistant')
BEGIN
    INSERT INTO session_message_counts (session_id, count, updated_at)
    VALUES (NEW.session_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(session_id) DO UPDATE SET
        count = count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;
"#;

/// Sprint 3: Trigger to track when summaries are created
const CREATE_SUMMARY_TRACKING_TRIGGER: &str = r#"
CREATE TRIGGER IF NOT EXISTS track_summary_creation
AFTER INSERT ON chat_history
WHEN NEW.tags LIKE '%summary:rolling:%'
BEGIN
    UPDATE session_message_counts
    SET 
        last_summary_10_at = CASE 
            WHEN NEW.tags LIKE '%summary:rolling:10%' THEN 
                (SELECT COUNT(*) FROM chat_history WHERE session_id = NEW.session_id AND role IN ('user', 'assistant'))
            ELSE last_summary_10_at
        END,
        last_summary_100_at = CASE 
            WHEN NEW.tags LIKE '%summary:rolling:100%' THEN 
                (SELECT COUNT(*) FROM chat_history WHERE session_id = NEW.session_id AND role IN ('user', 'assistant'))
            ELSE last_summary_100_at
        END,
        updated_at = CURRENT_TIMESTAMP
    WHERE session_id = NEW.session_id;
    
    -- Also insert into summary_metadata for tracking
    INSERT INTO summary_metadata (session_id, summary_type, message_count_at_creation, summary_entry_id)
    VALUES (
        NEW.session_id,
        CASE 
            WHEN NEW.tags LIKE '%summary:rolling:10%' THEN 'rolling_10'
            WHEN NEW.tags LIKE '%summary:rolling:100%' THEN 'rolling_100'
            ELSE 'snapshot'
        END,
        (SELECT COUNT(*) FROM chat_history WHERE session_id = NEW.session_id AND role IN ('user', 'assistant')),
        last_insert_rowid()
    );
END;
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

/// Check if a table exists
async fn table_exists(pool: &SqlitePool, table: &str) -> Result<bool> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
    )
    .bind(table)
    .fetch_one(pool)
    .await?;
    Ok(exists > 0)
}

/// Check if a trigger exists
async fn trigger_exists(pool: &SqlitePool, trigger: &str) -> Result<bool> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name=?",
    )
    .bind(trigger)
    .fetch_one(pool)
    .await?;
    Ok(exists > 0)
}

/// Runs all required migrations for SQLite backend. Idempotent.
pub async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    println!("🔧 Running SQLite migrations...");
    
    // Base tables
    pool.execute(CREATE_CHAT_HISTORY).await?;
    pool.execute(CREATE_PROJECTS).await?;
    pool.execute(CREATE_ARTIFACTS).await?;
    pool.execute(CREATE_GIT_REPO_ATTACHMENTS).await?;

    // Sprint 3: Session message counting tables
    if !table_exists(pool, "session_message_counts").await? {
        println!("📊 Creating session_message_counts table for rolling summaries...");
        pool.execute(CREATE_SESSION_MESSAGE_COUNTS).await?;
        
        // Initialize counts from existing data
        println!("📊 Initializing message counts from existing conversations...");
        pool.execute(INITIALIZE_MESSAGE_COUNTS).await?;
    }
    
    if !table_exists(pool, "summary_metadata").await? {
        println!("📊 Creating summary_metadata table for tracking summaries...");
        pool.execute(CREATE_SUMMARY_METADATA).await?;
    }

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
    
    // Sprint 3: Create triggers for automatic counting
    if !trigger_exists(pool, "update_message_count_on_insert").await? {
        println!("🔧 Creating message count trigger...");
        pool.execute(CREATE_MESSAGE_COUNT_TRIGGER).await?;
    }
    
    if !trigger_exists(pool, "track_summary_creation").await? {
        println!("🔧 Creating summary tracking trigger...");
        pool.execute(CREATE_SUMMARY_TRACKING_TRIGGER).await?;
    }

    println!("✅ SQLite migrations complete!");
    Ok(())
}
