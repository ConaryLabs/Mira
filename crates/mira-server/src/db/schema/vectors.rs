// crates/mira-server/src/db/schema/vectors.rs
// Vector table migrations for embeddings storage

use crate::db::{get_server_state_sync, set_server_state_sync};
use crate::embeddings::EMBEDDING_PROVIDER_KEY;
use anyhow::Result;
use rusqlite::Connection;

/// Migrate vector tables if dimensions changed (legacy migration — runs once)
pub fn migrate_vec_tables(conn: &Connection) -> Result<()> {
    // This migration handled the initial 768→1536 change.
    // Dynamic dimension management is now handled by ensure_vec_table_dimensions().
    let _ = conn;
    Ok(())
}

/// Parse the current dimension of vec_memory from its schema SQL.
fn current_vec_memory_dims(conn: &Connection) -> Option<usize> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='vec_memory'",
        [],
        |row| {
            let sql: String = row.get(0)?;
            // Parse dimension from SQL like "embedding float[1536]"
            if let Some(start) = sql.find("float[") {
                let rest = &sql[start + 6..];
                if let Some(end) = rest.find(']')
                    && let Ok(dim) = rest[..end].parse::<usize>()
                {
                    return Ok(Some(dim));
                }
            }
            Ok(None)
        },
    )
    .unwrap_or(None)
}

/// Ensure vec_memory table dimensions match the active embedding provider.
///
/// Call this at startup after the embedding client is created. If the table
/// dimensions don't match `target_dims`, drops and recreates vec_memory with
/// the correct dimensions, then resets has_embedding flags for re-embedding.
pub fn ensure_vec_table_dimensions(conn: &Connection, target_dims: usize) -> Result<()> {
    let current = current_vec_memory_dims(conn);

    match current {
        Some(dim) if dim == target_dims => {
            // Already correct
            Ok(())
        }
        Some(dim) => {
            tracing::info!(
                "vec_memory dimensions mismatch ({} -> {}), recreating table",
                dim,
                target_dims
            );
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch("DROP TABLE IF EXISTS vec_memory")?;
            tx.execute_batch(&format!(
                "CREATE VIRTUAL TABLE vec_memory USING vec0(\
                     embedding float[{target_dims}],\
                     +fact_id INTEGER,\
                     +content TEXT\
                 )"
            ))?;
            tx.execute("UPDATE memory_facts SET has_embedding = 0", [])?;
            tx.commit()?;
            Ok(())
        }
        None => {
            // Table doesn't exist yet — create with correct dimensions
            tracing::info!("Creating vec_memory with {} dimensions", target_dims);
            conn.execute_batch(&format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS vec_memory USING vec0(\
                     embedding float[{target_dims}],\
                     +fact_id INTEGER,\
                     +content TEXT\
                 )"
            ))?;
            Ok(())
        }
    }
}

/// Check if the embedding provider has changed and invalidate vec_memory if so.
///
/// Reads the stored provider from `server_state`. If it differs from `current_provider`
/// (or is absent on first run), clears `vec_memory` and resets `has_embedding` flags
/// so the background worker will re-embed all memory facts.
///
/// Returns `true` if vec_memory was invalidated (re-embedding needed).
pub fn check_embedding_provider_change(conn: &Connection, current_provider: &str) -> Result<bool> {
    let stored = get_server_state_sync(conn, EMBEDDING_PROVIDER_KEY).unwrap_or(None);

    if stored.as_deref() == Some(current_provider) {
        return Ok(false);
    }

    let old = stored.as_deref().unwrap_or("unknown");
    tracing::info!(
        "Embedding provider changed ({} -> {}), clearing vec_memory",
        old,
        current_provider
    );

    let tx = conn.unchecked_transaction()?;

    // Clear all memory embeddings
    tx.execute_batch("DELETE FROM vec_memory")?;

    // Reset has_embedding flags so background worker re-embeds
    tx.execute("UPDATE memory_facts SET has_embedding = 0", [])?;

    // Store the new provider
    set_server_state_sync(&tx, EMBEDDING_PROVIDER_KEY, current_provider)?;
    tx.commit()?;

    Ok(true)
}

// Note: vec_code and pending_embeddings migrations are now in db/schema/code.rs
// (they apply to the separate code database, not the main database)
