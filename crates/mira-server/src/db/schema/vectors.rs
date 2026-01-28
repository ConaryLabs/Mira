// crates/mira-server/src/db/schema/vectors.rs
// Vector table migrations for embeddings storage

use crate::db::migration_helpers::{add_column_if_missing, table_exists};
use anyhow::Result;
use rusqlite::Connection;

/// Migrate vector tables if dimensions changed
pub fn migrate_vec_tables(conn: &Connection) -> Result<()> {
    // Check if vec_memory exists by examining its SQL definition
    let current_dim: Option<i64> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='vec_memory'",
            [],
            |row| {
                let sql: String = row.get(0)?;
                // Parse dimension from SQL like "embedding float[1536]"
                if let Some(start) = sql.find("float[") {
                    let rest = &sql[start + 6..];
                    if let Some(end) = rest.find(']') {
                        if let Ok(dim) = rest[..end].parse::<i64>() {
                            return Ok(Some(dim));
                        }
                    }
                }
                Ok(None)
            },
        )
        .unwrap_or(None);

    if let Some(dim) = current_dim {
        if dim != 1536 {
            tracing::info!("Migrating vector tables from {} to 1536 dimensions", dim);
            // Drop old tables - CASCADE not supported, drop in order
            conn.execute_batch(
                "DROP TABLE IF EXISTS vec_memory;
                 DROP TABLE IF EXISTS vec_code;",
            )?;
        }
    }

    Ok(())
}

/// Migrate vec_code to add start_line column (v2.1 schema)
/// Also creates vec_code if it doesn't exist (for databases created before vec_code was added)
pub fn migrate_vec_code_line_numbers(conn: &Connection) -> Result<()> {
    // Check if vec_code exists
    let vec_code_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='vec_code'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !vec_code_exists {
        // Create vec_code table (for databases created before this table was added to schema)
        tracing::info!("Creating vec_code table for code embeddings");
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(
                embedding float[1536],
                +file_path TEXT,
                +chunk_content TEXT,
                +project_id INTEGER,
                +start_line INTEGER
            )",
            [],
        )?;
        return Ok(());
    }

    // Check if start_line column exists by examining the table's SQL definition
    // (vec0 virtual tables don't expose column metadata in their _info tables)
    let has_start_line: bool = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='vec_code'",
            [],
            |row| {
                let sql: String = row.get(0)?;
                Ok(sql.contains("start_line"))
            },
        )
        .unwrap_or(false);

    if !has_start_line {
        tracing::info!("Migrating vec_code to add start_line column");
        // Virtual tables can't be altered - must drop and recreate
        // Embeddings will be regenerated on next indexing
        conn.execute("DROP TABLE IF EXISTS vec_code", [])?;
        // Recreate with start_line column
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(
                embedding float[1536],
                +file_path TEXT,
                +chunk_content TEXT,
                +project_id INTEGER,
                +start_line INTEGER
            )",
            [],
        )?;
    }

    Ok(())
}

/// Migrate pending_embeddings to add start_line column
pub fn migrate_pending_embeddings_line_numbers(conn: &Connection) -> Result<()> {
    // Early return if table doesn't exist
    if !table_exists(conn, "pending_embeddings") {
        return Ok(());
    }

    // Add column if missing
    add_column_if_missing(
        conn,
        "pending_embeddings",
        "start_line",
        "INTEGER NOT NULL DEFAULT 1",
    )
}
