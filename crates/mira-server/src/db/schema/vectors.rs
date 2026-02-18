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

/// Invalidate vec_code in the code database after an embedding provider change.
///
/// Clears `vec_code` (semantic embeddings only — `code_fts` is provider-independent
/// and is preserved for uninterrupted keyword search). Re-populates `pending_embeddings`
/// from all existing `code_chunks` so the fast-lane worker immediately begins re-embedding
/// with the new provider. The canonical `code_chunks` table is always preserved.
pub fn invalidate_code_embeddings(code_conn: &Connection) -> Result<()> {
    tracing::info!("Clearing vec_code and re-queuing all code chunks for re-embedding");

    let tx = code_conn.unchecked_transaction()?;
    tx.execute_batch("DELETE FROM vec_code")?;

    // Clear any existing pending entries, then insert all chunks exactly once.
    // pending_embeddings has no UNIQUE constraint, so INSERT OR IGNORE is a no-op;
    // DELETE + INSERT is the correct way to get an exact, duplicate-free queue.
    tx.execute_batch("DELETE FROM pending_embeddings WHERE status = 'pending'")?;
    tx.execute_batch(
        "INSERT INTO pending_embeddings (project_id, file_path, chunk_content, start_line)
         SELECT project_id, file_path, chunk_content, start_line FROM code_chunks",
    )?;

    tx.commit()?;

    let queued: i64 = code_conn
        .query_row(
            "SELECT COUNT(*) FROM pending_embeddings WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    tracing::info!("Queued {} code chunks for re-embedding", queued);

    Ok(())
}

/// Parse the current dimension of vec_code from its schema SQL.
pub(crate) fn current_vec_code_dims(conn: &Connection) -> Option<usize> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='vec_code'",
        [],
        |row| {
            let sql: String = row.get(0)?;
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

/// Ensure vec_code table dimensions match the active embedding provider.
///
/// Call this after the code database is opened and embedding dimensions are known.
/// If the table dimensions don't match `target_dims`, drops and recreates vec_code
/// with the correct dimensions. The canonical code_chunks and FTS tables are always
/// preserved — only the vector index is dropped.
pub fn ensure_code_vec_table_dimensions(conn: &Connection, target_dims: usize) -> Result<()> {
    let current = current_vec_code_dims(conn);

    match current {
        Some(dim) if dim == target_dims => {
            // Already correct
            Ok(())
        }
        Some(dim) => {
            tracing::info!(
                "vec_code dimensions mismatch ({} -> {}), recreating table",
                dim,
                target_dims
            );
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch("DROP TABLE IF EXISTS vec_code")?;
            tx.execute_batch(&format!(
                "CREATE VIRTUAL TABLE vec_code USING vec0(\
                     embedding float[{target_dims}],\
                     +file_path TEXT,\
                     +chunk_content TEXT,\
                     +project_id INTEGER,\
                     +start_line INTEGER,\
                     chunk_size=256\
                 )"
            ))?;
            tx.commit()?;
            Ok(())
        }
        None => {
            // Table doesn't exist yet — will be created by run_code_migrations
            Ok(())
        }
    }
}

/// Recovery check: if vec_code is empty but code_chunks exist and nothing is queued,
/// re-populate pending_embeddings. Called at startup to handle cases where a previous
/// invalidation succeeded in clearing vec_code but failed before re-queuing (e.g. crash).
pub fn ensure_code_embeddings_queued(code_conn: &Connection) -> Result<()> {
    let vec_count: i64 = code_conn
        .query_row("SELECT COUNT(*) FROM vec_code", [], |r| r.get(0))
        .unwrap_or(1); // default 1 = assume present if query fails

    let chunk_count: i64 = code_conn
        .query_row("SELECT COUNT(*) FROM code_chunks", [], |r| r.get(0))
        .unwrap_or(0);

    let pending_count: i64 = code_conn
        .query_row(
            "SELECT COUNT(*) FROM pending_embeddings WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if vec_count == 0 && chunk_count > 0 && pending_count == 0 {
        tracing::warn!(
            "vec_code is empty but {} code chunks exist with nothing queued — \
             re-queuing for re-embedding (likely from a previous interrupted provider switch)",
            chunk_count
        );
        // pending_count == 0 is verified above, so a plain INSERT is duplicate-free.
        code_conn.execute_batch(
            "INSERT INTO pending_embeddings (project_id, file_path, chunk_content, start_line)
             SELECT project_id, file_path, chunk_content, start_line FROM code_chunks",
        )?;
    }

    Ok(())
}

// Note: vec_code and pending_embeddings migrations are now in db/schema/code.rs
// (they apply to the separate code database, not the main database)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::code::vec_code_create_sql;

    /// Open an in-memory connection with sqlite-vec loaded but WITHOUT running main-db
    /// migrations — simulates a bare code database as opened by the indexer.
    fn code_conn() -> Connection {
        use crate::db::pool::ensure_sqlite_vec_registered;
        ensure_sqlite_vec_registered();
        Connection::open_in_memory().unwrap()
    }

    // ─── vec_code_create_sql ───────────────────────────────────────────────────

    #[test]
    fn test_vec_code_create_sql_embeds_given_dim() {
        assert!(vec_code_create_sql(768).contains("float[768]"));
    }

    #[test]
    fn test_vec_code_create_sql_1536_roundtrip() {
        assert!(vec_code_create_sql(1536).contains("float[1536]"));
    }

    // ─── current_vec_code_dims ────────────────────────────────────────────────

    #[test]
    fn test_current_vec_code_dims_returns_dim_from_ddl() {
        let conn = code_conn();
        conn.execute_batch(&vec_code_create_sql(768)).unwrap();
        assert_eq!(current_vec_code_dims(&conn), Some(768));
    }

    #[test]
    fn test_current_vec_code_dims_1536() {
        let conn = code_conn();
        conn.execute_batch(&vec_code_create_sql(1536)).unwrap();
        assert_eq!(current_vec_code_dims(&conn), Some(1536));
    }

    #[test]
    fn test_current_vec_code_dims_absent_returns_none() {
        let conn = code_conn();
        // No vec_code table created — should return None, not panic
        assert_eq!(current_vec_code_dims(&conn), None);
    }

    // ─── ensure_code_vec_table_dimensions ─────────────────────────────────────

    #[test]
    fn test_ensure_dims_match_is_noop() {
        let conn = code_conn();
        conn.execute_batch(&vec_code_create_sql(768)).unwrap();
        ensure_code_vec_table_dimensions(&conn, 768).unwrap();
        // Table still present with unchanged dimension
        assert_eq!(current_vec_code_dims(&conn), Some(768));
    }

    #[test]
    fn test_ensure_dims_mismatch_recreates_with_target() {
        let conn = code_conn();
        conn.execute_batch(&vec_code_create_sql(1536)).unwrap();
        ensure_code_vec_table_dimensions(&conn, 768).unwrap();
        assert_eq!(current_vec_code_dims(&conn), Some(768));
    }

    #[test]
    fn test_ensure_dims_mismatch_old_dim_gone() {
        let conn = code_conn();
        conn.execute_batch(&vec_code_create_sql(1536)).unwrap();
        ensure_code_vec_table_dimensions(&conn, 512).unwrap();
        // Must not be 1536 anymore
        assert_ne!(current_vec_code_dims(&conn), Some(1536));
    }

    #[test]
    fn test_ensure_dims_absent_is_noop() {
        let conn = code_conn();
        // No vec_code — should succeed and leave table absent
        ensure_code_vec_table_dimensions(&conn, 768).unwrap();
        assert_eq!(current_vec_code_dims(&conn), None);
    }
}
