// crates/mira-server/src/db/index.rs
// Database operations for code index management
//
// Consolidates index-related SQL from:
// - indexer/mod.rs (clear_existing_project_data)
// - background/watcher.rs (file deletion)
// - tools/core/code.rs (counts, module cleanup)

use rusqlite::{params, Connection};

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
        "DELETE FROM vec_code WHERE project_id = ?",
        params![project_id],
    )?;

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
pub fn clear_file_index_sync(conn: &Connection, project_id: i64, file_path: &str) -> rusqlite::Result<()> {
    // Delete symbols for this file
    conn.execute(
        "DELETE FROM code_symbols WHERE project_id = ? AND file_path = ?",
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
pub fn clear_modules_without_purpose_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM codebase_modules WHERE project_id = ? AND purpose IS NULL",
        params![project_id],
    )?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn test_count_symbols_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let count = count_symbols_sync(&conn, None);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_count_symbols_with_project() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let count = count_symbols_sync(&conn, Some(1));
        assert_eq!(count, 0);
    }

    #[test]
    fn test_count_embedded_chunks_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let count = count_embedded_chunks_sync(&conn, None);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_count_embedded_chunks_with_project() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let count = count_embedded_chunks_sync(&conn, Some(1));
        assert_eq!(count, 0);
    }

    #[test]
    fn test_clear_project_index_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        // Should not error on empty tables
        let result = clear_project_index_sync(&conn, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_clear_file_index_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        // Should not error on empty tables
        let result = clear_file_index_sync(&conn, 1, "src/main.rs");
        assert!(result.is_ok());
    }

    #[test]
    fn test_clear_modules_without_purpose_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let deleted = clear_modules_without_purpose_sync(&conn, 1).unwrap();
        assert_eq!(deleted, 0);
    }
}
