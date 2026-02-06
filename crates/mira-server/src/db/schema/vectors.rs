// crates/mira-server/src/db/schema/vectors.rs
// Vector table migrations for embeddings storage

use crate::db::{get_server_state_sync, set_server_state_sync};
use crate::embeddings::EMBEDDING_PROVIDER_KEY;
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
                    if let Some(end) = rest.find(']')
                        && let Ok(dim) = rest[..end].parse::<i64>()
                    {
                        return Ok(Some(dim));
                    }
                }
                Ok(None)
            },
        )
        .unwrap_or(None);

    if let Some(dim) = current_dim
        && dim != 1536
    {
        tracing::info!("Migrating vector tables from {} to 1536 dimensions", dim);
        // Drop old tables - CASCADE not supported, drop in order
        conn.execute_batch(
            "DROP TABLE IF EXISTS vec_memory;
                 DROP TABLE IF EXISTS vec_code;",
        )?;
    }

    Ok(())
}

/// Check if the embedding provider has changed and invalidate vec_memory if so.
///
/// Reads the stored provider from `server_state`. If it differs from `current_provider`
/// (or is absent on first run), clears `vec_memory` and resets `has_embedding` flags
/// so the background worker will re-embed all memory facts.
///
/// Returns `true` if vec_memory was invalidated (re-embedding needed).
pub fn check_embedding_provider_change(
    conn: &Connection,
    current_provider: &str,
) -> Result<bool> {
    let stored = get_server_state_sync(conn, EMBEDDING_PROVIDER_KEY)
        .unwrap_or(None);

    if stored.as_deref() == Some(current_provider) {
        return Ok(false);
    }

    let old = stored.as_deref().unwrap_or("unknown");
    tracing::info!(
        "Embedding provider changed ({} -> {}), clearing vec_memory",
        old,
        current_provider
    );

    // Clear all memory embeddings
    conn.execute_batch("DELETE FROM vec_memory")?;

    // Reset has_embedding flags so background worker re-embeds
    conn.execute("UPDATE memory_facts SET has_embedding = 0", [])?;

    // Store the new provider
    set_server_state_sync(conn, EMBEDDING_PROVIDER_KEY, current_provider)?;

    Ok(true)
}

// Note: vec_code and pending_embeddings migrations are now in db/schema/code.rs
// (they apply to the separate code database, not the main database)
