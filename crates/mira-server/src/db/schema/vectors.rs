// crates/mira-server/src/db/schema/vectors.rs
// Vector table migrations for embeddings storage

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

// Note: vec_code and pending_embeddings migrations are now in db/schema/code.rs
// (they apply to the separate code database, not the main database)
