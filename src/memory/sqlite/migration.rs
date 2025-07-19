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

/// Runs all required migrations for SQLite backend.
/// Safe to call at every startup (idempotent).
pub async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    pool.execute(CREATE_CHAT_HISTORY).await?;
    // Add any ALTER TABLE migrations here as you add new fields in the future.
    Ok(())
}
