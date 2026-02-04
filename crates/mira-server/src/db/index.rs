// crates/mira-server/src/db/index.rs
// Database operations for code index management
//
// Consolidates index-related SQL from:
// - indexer/mod.rs (clear_existing_project_data)
// - background/watcher.rs (file deletion)
// - tools/core/code.rs (counts, module cleanup)

use crate::db::schema::code::VEC_CODE_CREATE_SQL;
use rusqlite::{Connection, params};

/// (embedding, file_path, chunk_content, project_id, start_line)
type EmbeddingRow = (Vec<u8>, String, String, Option<i64>, i64);

/// Clear all index data for a project (symbols, embeddings, imports, modules, call graph)
///
/// This is used when re-indexing a project from scratch.
/// Order matters: call_graph references code_symbols, so delete it first.
pub fn clear_project_index_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    // Delete call_graph first (references code_symbols via caller_id)
    conn.execute(
        "DELETE FROM call_graph WHERE caller_id IN (SELECT id FROM code_symbols WHERE project_id = ?)",
        params![project_id],
    )?;

    conn.execute(
        "DELETE FROM code_symbols WHERE project_id = ?",
        params![project_id],
    )?;

    conn.execute(
        "DELETE FROM code_chunks WHERE project_id = ?",
        params![project_id],
    )?;

    conn.execute(
        "DELETE FROM code_fts WHERE project_id = ?",
        params![project_id],
    )?;

    // For vec_code: DROP+recreate if this is the only project to reclaim
    // sqlite-vec chunk storage. Otherwise DELETE as usual.
    let other_project_vectors: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vec_code WHERE project_id != ?",
            params![project_id],
            |r| r.get(0),
        )
        .unwrap_or(1); // Default to 1 (safe path) on error

    if other_project_vectors == 0 {
        conn.execute("DROP TABLE IF EXISTS vec_code", [])?;
        conn.execute(VEC_CODE_CREATE_SQL, [])?;
    } else {
        conn.execute(
            "DELETE FROM vec_code WHERE project_id = ?",
            params![project_id],
        )?;
    }

    conn.execute(
        "DELETE FROM imports WHERE project_id = ?",
        params![project_id],
    )?;

    conn.execute(
        "DELETE FROM codebase_modules WHERE project_id = ?",
        params![project_id],
    )?;

    Ok(())
}

/// Clear index data for a specific file within a project
///
/// Used by the file watcher when a file is deleted or needs re-indexing.
pub fn clear_file_index_sync(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
) -> rusqlite::Result<()> {
    // Delete symbols for this file
    conn.execute(
        "DELETE FROM code_symbols WHERE project_id = ? AND file_path = ?",
        params![project_id, file_path],
    )?;

    // Delete code chunks for this file
    conn.execute(
        "DELETE FROM code_chunks WHERE project_id = ? AND file_path = ?",
        params![project_id, file_path],
    )?;

    conn.execute(
        "DELETE FROM code_fts WHERE project_id = ? AND file_path = ?",
        params![project_id, file_path],
    )?;

    // Delete embeddings for this file
    conn.execute(
        "DELETE FROM vec_code WHERE project_id = ? AND file_path = ?",
        params![project_id, file_path],
    )?;

    // Delete imports for this file
    conn.execute(
        "DELETE FROM imports WHERE project_id = ? AND file_path = ?",
        params![project_id, file_path],
    )?;

    Ok(())
}

/// Count code symbols for a project (or all projects if None)
pub fn count_symbols_sync(conn: &Connection, project_id: Option<i64>) -> i64 {
    if let Some(pid) = project_id {
        conn.query_row(
            "SELECT COUNT(*) FROM code_symbols WHERE project_id = ?",
            [pid],
            |r| r.get(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row("SELECT COUNT(*) FROM code_symbols", [], |r| r.get(0))
            .unwrap_or(0)
    }
}

/// Count embedded code chunks for a project (or all projects if None)
pub fn count_embedded_chunks_sync(conn: &Connection, project_id: Option<i64>) -> i64 {
    if let Some(pid) = project_id {
        conn.query_row(
            "SELECT COUNT(*) FROM vec_code WHERE project_id = ?",
            [pid],
            |r| r.get(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row("SELECT COUNT(*) FROM vec_code", [], |r| r.get(0))
            .unwrap_or(0)
    }
}

/// Clear cached modules that don't have a purpose set
///
/// Used after generating module summaries to clean up partial entries.
pub fn clear_modules_without_purpose_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM codebase_modules WHERE project_id = ? AND purpose IS NULL",
        params![project_id],
    )?;
    Ok(deleted)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Vector table compaction
// ═══════════════════════════════════════════════════════════════════════════════

/// Statistics from a vec_code compact operation.
///
/// `estimated_savings_mb` is calculated from chunk count differences, not
/// measured file size — the actual file won't shrink until VACUUM runs.
pub struct CompactStats {
    pub rows_preserved: usize,
    pub estimated_savings_mb: f64,
}

/// Compact vec_code by extracting all rows, dropping the table, and recreating
/// with chunk_size=256.
///
/// sqlite-vec's `vec0` uses fixed-size chunks (default 1024 vectors, ~6 MB each).
/// DELETEs mark slots invalid but don't release chunk storage. This function
/// reclaims that wasted space.
///
/// ROWID safety: vec_code rowids are not referenced by FTS (uses `code_chunks.id`)
/// or any other table, so reassignment during reinsert is safe.
pub fn compact_vec_code_sync(conn: &Connection) -> rusqlite::Result<CompactStats> {
    let row_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_code", [], |r| r.get(0))
        .unwrap_or(0);

    if row_count == 0 {
        // Nothing to preserve — just recreate
        conn.execute("DROP TABLE IF EXISTS vec_code", [])?;
        conn.execute(VEC_CODE_CREATE_SQL, [])?;
        return Ok(CompactStats {
            rows_preserved: 0,
            estimated_savings_mb: 0.0,
        });
    }

    // Extract all embeddings into memory (~6 KB per row)
    let mut stmt = conn.prepare(
        "SELECT embedding, file_path, chunk_content, project_id, start_line FROM vec_code",
    )?;
    let rows: Vec<EmbeddingRow> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let preserved = rows.len();

    // Estimate savings: each old chunk is ~6 MB (1024 slots * 6 KB),
    // new chunks are ~1.5 MB (256 slots). Calculate old vs new chunk counts.
    let old_chunks = (preserved as f64 / 1024.0).ceil();
    let new_chunks = (preserved as f64 / 256.0).ceil();
    // But the real savings come from empty chunks that had accumulated
    // We estimate based on the total chunk count from sqlite_master
    let old_total_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name LIKE 'vec_code_vector_chunks%'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(old_chunks as i64);
    let estimated_savings = (old_total_chunks as f64 * 6.0) - (new_chunks * 1.5);

    // DROP and recreate with optimized chunk_size
    conn.execute("DROP TABLE IF EXISTS vec_code", [])?;
    conn.execute(VEC_CODE_CREATE_SQL, [])?;

    // Re-insert all rows in a transaction with prepared statement
    let tx = conn.unchecked_transaction()?;
    {
        let mut insert_stmt = tx.prepare(
            "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id, start_line) VALUES (?, ?, ?, ?, ?)",
        )?;
        for (embedding, file_path, chunk_content, project_id, start_line) in &rows {
            insert_stmt.execute(params![
                embedding,
                file_path,
                chunk_content,
                project_id,
                start_line
            ])?;
        }
    }
    tx.commit()?;

    Ok(CompactStats {
        rows_preserved: preserved,
        estimated_savings_mb: estimated_savings.max(0.0),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Batch insert operations for indexing
// ═══════════════════════════════════════════════════════════════════════════════

/// Symbol data for batch insertion
pub struct SymbolInsert<'a> {
    pub name: &'a str,
    pub symbol_type: &'a str,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<&'a str>,
}

/// Import data for batch insertion
pub struct ImportInsert<'a> {
    pub import_path: &'a str,
    pub is_external: bool,
}

/// Insert a symbol and return its ID
/// Uses transaction for batch operations
pub fn insert_symbol_sync(
    tx: &rusqlite::Transaction,
    project_id: Option<i64>,
    file_path: &str,
    sym: &SymbolInsert,
) -> rusqlite::Result<i64> {
    tx.execute(
        "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line, signature)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            project_id,
            file_path,
            sym.name,
            sym.symbol_type,
            sym.start_line,
            sym.end_line,
            sym.signature
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Insert an import (ignores duplicates)
/// Uses transaction for batch operations
pub fn insert_import_sync(
    tx: &rusqlite::Transaction,
    project_id: Option<i64>,
    file_path: &str,
    import: &ImportInsert,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO imports (project_id, file_path, import_path, is_external)
         VALUES (?, ?, ?, ?)",
        params![
            project_id,
            file_path,
            import.import_path,
            import.is_external as i32
        ],
    )?;
    Ok(())
}

/// Insert a function call into the call graph
/// Uses transaction for batch operations
pub fn insert_call_sync(
    tx: &rusqlite::Transaction,
    caller_id: i64,
    callee_name: &str,
    callee_id: Option<i64>,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO call_graph (caller_id, callee_name, callee_id)
         VALUES (?, ?, ?)",
        params![caller_id, callee_name, callee_id],
    )?;
    Ok(())
}

/// Insert a code chunk embedding
/// Uses transaction for batch operations
pub fn insert_chunk_embedding_sync(
    tx: &rusqlite::Transaction,
    embedding_bytes: &[u8],
    file_path: &str,
    content: &str,
    project_id: Option<i64>,
    start_line: usize,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id, start_line)
         VALUES (?, ?, ?, ?, ?)",
        params![embedding_bytes, file_path, content, project_id, start_line],
    )?;
    Ok(())
}

/// Insert a code chunk into the canonical code_chunks table
/// Uses transaction for batch operations
pub fn insert_code_chunk_sync(
    tx: &rusqlite::Transaction,
    project_id: Option<i64>,
    file_path: &str,
    chunk_content: &str,
    start_line: u32,
) -> rusqlite::Result<i64> {
    tx.execute(
        "INSERT INTO code_chunks (project_id, file_path, chunk_content, start_line)
         VALUES (?, ?, ?, ?)",
        params![project_id, file_path, chunk_content, start_line],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Insert a code chunk into the FTS index using a specific rowid
/// Uses transaction for batch operations
pub fn insert_code_fts_entry_sync(
    tx: &rusqlite::Transaction,
    rowid: i64,
    file_path: &str,
    chunk_content: &str,
    project_id: Option<i64>,
    start_line: u32,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO code_fts (rowid, file_path, chunk_content, project_id, start_line)
         VALUES (?, ?, ?, ?, ?)",
        params![rowid, file_path, chunk_content, project_id, start_line],
    )?;
    Ok(())
}

/// Queue a chunk for background embedding processing
pub fn queue_pending_embedding_sync(
    tx: &rusqlite::Transaction,
    project_id: Option<i64>,
    file_path: &str,
    chunk_content: &str,
    start_line: u32,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO pending_embeddings (project_id, file_path, chunk_content, start_line, status)
         VALUES (?, ?, ?, ?, 'pending')",
        params![project_id, file_path, chunk_content, start_line],
    )?;
    Ok(())
}
