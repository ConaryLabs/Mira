// crates/mira-server/src/db/background.rs
// Database operations for background workers (capabilities, health, documentation)

use rusqlite::{params, Connection};

// ═══════════════════════════════════════════════════════════════════════════════
// Common scan time/rate limiting functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Get scan info (content/commit, updated_at) for a memory key
pub fn get_scan_info_sync(
    conn: &Connection,
    project_id: i64,
    key: &str,
) -> Option<(String, String)> {
    conn.query_row(
        "SELECT content, updated_at FROM memory_facts
         WHERE project_id = ? AND key = ?",
        params![project_id, key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .ok()
}

/// Check if a time is older than a duration (e.g., '-1 day', '-7 days', '-1 hour')
pub fn is_time_older_than_sync(conn: &Connection, time: &str, duration: &str) -> bool {
    conn.query_row(
        &format!("SELECT datetime(?) < datetime('now', '{}')", duration),
        [time],
        |row| row.get(0),
    )
    .unwrap_or(false)
}

/// Check if a memory key exists for a project
pub fn memory_key_exists_sync(conn: &Connection, project_id: i64, key: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM memory_facts WHERE project_id = ? AND key = ?",
        params![project_id, key],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

/// Delete a memory by key
pub fn delete_memory_by_key_sync(
    conn: &Connection,
    project_id: i64,
    key: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM memory_facts WHERE project_id = ? AND key = ?",
        params![project_id, key],
    )
}

/// Insert a system memory marker (for scan times, flags)
pub fn insert_system_marker_sync(
    conn: &Connection,
    project_id: i64,
    key: &str,
    content: &str,
    category: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO memory_facts (project_id, key, content, fact_type, category, confidence, created_at, updated_at)
         VALUES (?, ?, ?, 'system', ?, 1.0, datetime('now'), datetime('now'))",
        params![project_id, key, content, category],
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Capabilities scanner
// ═══════════════════════════════════════════════════════════════════════════════

/// Clear old capabilities for a project
pub fn clear_old_capabilities_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM memory_facts WHERE project_id = ? AND fact_type = 'capability' AND category = 'codebase'",
        [project_id],
    )?;
    // Clean up orphaned embeddings
    conn.execute(
        "DELETE FROM vec_memory WHERE fact_id NOT IN (SELECT id FROM memory_facts)",
        [],
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Health scanner
// ═══════════════════════════════════════════════════════════════════════════════

/// Mark project as health-scanned and clear the "needs scan" flag
pub fn mark_health_scanned_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    // Clear the "needs scan" flag
    conn.execute(
        "DELETE FROM memory_facts WHERE project_id = ? AND key = 'health_scan_needed'",
        [project_id],
    )?;
    // Update last scan time
    conn.execute(
        "INSERT OR REPLACE INTO memory_facts (project_id, key, content, fact_type, category, confidence, created_at, updated_at)
         VALUES (?, 'health_scan_time', 'scanned', 'system', 'health', 1.0, datetime('now'), datetime('now'))",
        [project_id],
    )?;
    Ok(())
}

/// Clear old health issues before refresh
pub fn clear_old_health_issues_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM memory_facts WHERE project_id = ? AND fact_type = 'health'",
        [project_id],
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Documentation scanner
// ═══════════════════════════════════════════════════════════════════════════════

/// Get documented items by category
pub fn get_documented_by_category_sync(
    conn: &Connection,
    project_id: i64,
    doc_category: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT doc_path FROM documentation_inventory
         WHERE project_id = ? AND doc_category = ?",
    )?;

    let paths = stmt
        .query_map(params![project_id, doc_category], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(paths)
}

/// Get symbols from lib.rs for public API gap detection
pub fn get_lib_symbols_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT name, signature
         FROM code_symbols
         WHERE project_id = ? AND file_path LIKE '%lib.rs'
         AND symbol_type IN ('function', 'struct', 'enum', 'type')
         ORDER BY name",
    )?;

    let symbols = stmt
        .query_map(params![project_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(symbols)
}

/// Get modules for documentation gap detection
pub fn get_modules_for_doc_gaps_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<(String, String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT module_id, path, purpose
         FROM codebase_modules
         WHERE project_id = ?
         ORDER BY module_id",
    )?;

    let modules = stmt
        .query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(modules)
}

/// Get symbols for a source file (for signature hash calculation)
pub fn get_symbols_for_file_sync(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
) -> rusqlite::Result<Vec<(i64, String, String, Option<i32>, Option<i32>, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, symbol_type, start_line, end_line, signature
         FROM code_symbols
         WHERE project_id = ? AND file_path = ?
         ORDER BY name",
    )?;

    let symbols = stmt
        .query_map(params![project_id, file_path], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(symbols)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Embeddings processor
// ═══════════════════════════════════════════════════════════════════════════════

/// Store a code embedding in vec_code
pub fn store_code_embedding_sync(
    conn: &Connection,
    embedding_bytes: &[u8],
    file_path: &str,
    chunk_content: &str,
    project_id: Option<i64>,
    start_line: usize,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id, start_line)
         VALUES (?, ?, ?, ?, ?)",
        params![embedding_bytes, file_path, chunk_content, project_id, start_line],
    )?;
    Ok(())
}

/// Delete a pending embedding by ID
pub fn delete_pending_embedding_sync(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM pending_embeddings WHERE id = ?", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn test_get_scan_info_none() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let result = get_scan_info_sync(&conn, 1, "test_key");
        assert!(result.is_none());
    }

    #[test]
    fn test_memory_key_exists_false() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        assert!(!memory_key_exists_sync(&conn, 1, "nonexistent"));
    }

    #[test]
    fn test_is_time_older_than() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        // Old time should be older than 1 day
        assert!(is_time_older_than_sync(&conn, "2020-01-01 00:00:00", "-1 day"));
    }

    #[test]
    fn test_clear_old_capabilities() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        // Should not error on empty db
        clear_old_capabilities_sync(&conn, 1).unwrap();
    }

    #[test]
    fn test_clear_old_health_issues() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        clear_old_health_issues_sync(&conn, 1).unwrap();
    }

    #[test]
    fn test_get_documented_by_category_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let result = get_documented_by_category_sync(&conn, 1, "mcp_tool").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_lib_symbols_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let result = get_lib_symbols_sync(&conn, 1).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_modules_for_doc_gaps_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let result = get_modules_for_doc_gaps_sync(&conn, 1).unwrap();
        assert!(result.is_empty());
    }
}
