// crates/mira-server/src/db/background.rs
// Database operations for background workers (capabilities, health, documentation)

use rusqlite::{Connection, params};

use super::log_and_discard;
use super::observations;

/// (id, name, symbol_type, start_line, end_line, signature)
pub type SymbolRow = (
    i64,
    String,
    String,
    Option<i32>,
    Option<i32>,
    Option<String>,
);

// ═══════════════════════════════════════════════════════════════════════════════
// Common scan time/rate limiting functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a time is older than a duration (e.g., '-1 day', '-7 days', '-1 hour')
pub fn is_time_older_than_sync(conn: &Connection, time: &str, duration: &str) -> bool {
    conn.query_row(
        "SELECT datetime(?1) < datetime('now', ?2)",
        params![time, duration],
        |row| row.get(0),
    )
    .unwrap_or(false)
}

/// Insert a system observation marker (for scan times, flags)
pub fn insert_system_marker_sync(
    conn: &Connection,
    project_id: i64,
    key: &str,
    content: &str,
    category: &str,
) -> rusqlite::Result<()> {
    observations::store_observation_sync(
        conn,
        observations::StoreObservationParams {
            project_id: Some(project_id),
            key: Some(key),
            content,
            observation_type: "system",
            category: Some(category),
            confidence: 1.0,
            source: "background",
            session_id: None,
            team_id: None,
            scope: "project",
            expires_at: None,
        },
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Health scanner
// ═══════════════════════════════════════════════════════════════════════════════

/// Mark project as health-scanned and clear the "needs scan" flag.
/// Also clears the fast-scan-done marker set by `process_health_fast_scans`.
pub fn mark_health_scanned_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    // Clear the "needs scan" flag
    conn.execute(
        "DELETE FROM system_observations WHERE project_id = ? AND key = 'health_scan_needed'",
        [project_id],
    )?;
    // Clear the fast-scan-done marker
    conn.execute(
        "DELETE FROM system_observations WHERE project_id = ? AND key = 'health_fast_scan_done'",
        [project_id],
    )?;
    // Update last scan time via insert_system_marker_sync (handles UPSERT)
    insert_system_marker_sync(conn, project_id, "health_scan_time", "scanned", "health")
}

/// Clear old health issues before refresh
pub fn clear_old_health_issues_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM system_observations WHERE project_id = ? AND observation_type = 'health'",
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
        .filter_map(log_and_discard)
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
        .filter_map(log_and_discard)
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
        .filter_map(log_and_discard)
        .collect();

    Ok(modules)
}

/// Get symbols for a source file (for signature hash calculation)
pub fn get_symbols_for_file_sync(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
) -> rusqlite::Result<Vec<SymbolRow>> {
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
        .filter_map(log_and_discard)
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
        params![
            embedding_bytes,
            file_path,
            chunk_content,
            project_id,
            start_line
        ],
    )?;
    Ok(())
}

/// Delete a pending embedding by ID
pub fn delete_pending_embedding_sync(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM pending_embeddings WHERE id = ?", [id])?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Code health analysis
// ═══════════════════════════════════════════════════════════════════════════════

/// Find functions that are never called (using indexed call graph)
/// Note: This is heuristic-based since the call graph doesn't capture self.method() calls
pub fn get_unused_functions_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<(String, String, i64)>> {
    // Find functions that are defined but never appear as callees
    // The call graph doesn't capture self.method() calls, so we use heuristics:
    // - Exclude common method patterns (process_*, handle_*, get_*, etc.)
    // - Exclude trait implementations and common entry points
    // - Exclude test functions
    let mut stmt = conn.prepare(
        "SELECT s.name, s.file_path, s.start_line
         FROM code_symbols s
         WHERE s.project_id = ?
           AND s.symbol_type = 'function'
           -- Not called anywhere in the call graph
           AND s.name NOT IN (SELECT DISTINCT callee_name FROM call_graph)
           -- Exclude test functions
           AND s.name NOT LIKE 'test_%'
           AND s.name NOT LIKE '%_test'
           AND s.name NOT LIKE '%_tests'
           AND s.file_path NOT LIKE '%/tests/%'
           AND s.file_path NOT LIKE '%_test.rs'
           -- Exclude common entry points and trait methods
           AND s.name NOT IN ('main', 'run', 'new', 'default', 'from', 'into', 'drop', 'clone', 'fmt', 'eq', 'hash', 'cmp', 'partial_cmp')
           -- Exclude common method patterns (likely called via self.*)
           AND s.name NOT LIKE 'process_%'
           AND s.name NOT LIKE 'handle_%'
           AND s.name NOT LIKE 'on_%'
           AND s.name NOT LIKE 'do_%'
           AND s.name NOT LIKE 'try_%'
           AND s.name NOT LIKE 'get_%'
           AND s.name NOT LIKE 'set_%'
           AND s.name NOT LIKE 'is_%'
           AND s.name NOT LIKE 'has_%'
           AND s.name NOT LIKE 'with_%'
           AND s.name NOT LIKE 'to_%'
           AND s.name NOT LIKE 'as_%'
           AND s.name NOT LIKE 'into_%'
           AND s.name NOT LIKE 'from_%'
           AND s.name NOT LIKE 'parse_%'
           AND s.name NOT LIKE 'build_%'
           AND s.name NOT LIKE 'create_%'
           AND s.name NOT LIKE 'make_%'
           AND s.name NOT LIKE 'init_%'
           AND s.name NOT LIKE 'setup_%'
           AND s.name NOT LIKE 'check_%'
           AND s.name NOT LIKE 'validate_%'
           AND s.name NOT LIKE 'clear_%'
           AND s.name NOT LIKE 'reset_%'
           AND s.name NOT LIKE 'update_%'
           AND s.name NOT LIKE 'delete_%'
           AND s.name NOT LIKE 'remove_%'
           AND s.name NOT LIKE 'add_%'
           AND s.name NOT LIKE 'insert_%'
           AND s.name NOT LIKE 'find_%'
           AND s.name NOT LIKE 'search_%'
           AND s.name NOT LIKE 'load_%'
           AND s.name NOT LIKE 'save_%'
           AND s.name NOT LIKE 'store_%'
           AND s.name NOT LIKE 'read_%'
           AND s.name NOT LIKE 'write_%'
           AND s.name NOT LIKE 'send_%'
           AND s.name NOT LIKE 'receive_%'
           AND s.name NOT LIKE 'start_%'
           AND s.name NOT LIKE 'stop_%'
           AND s.name NOT LIKE 'spawn_%'
           AND s.name NOT LIKE 'run_%'
           AND s.name NOT LIKE 'execute_%'
           AND s.name NOT LIKE 'render_%'
           AND s.name NOT LIKE 'format_%'
           AND s.name NOT LIKE 'generate_%'
           AND s.name NOT LIKE 'compute_%'
           AND s.name NOT LIKE 'calculate_%'
           AND s.name NOT LIKE 'mark_%'
           AND s.name NOT LIKE 'scan_%'
           AND s.name NOT LIKE 'index_%'
           AND s.name NOT LIKE 'register_%'
           AND s.name NOT LIKE 'unregister_%'
           AND s.name NOT LIKE 'connect_%'
           AND s.name NOT LIKE 'disconnect_%'
           AND s.name NOT LIKE 'open_%'
           AND s.name NOT LIKE 'close_%'
           AND s.name NOT LIKE 'lock_%'
           AND s.name NOT LIKE 'unlock_%'
           AND s.name NOT LIKE 'acquire_%'
           AND s.name NOT LIKE 'release_%'
           -- Exclude private helpers (underscore prefix)
           AND s.name NOT LIKE '_%'
         LIMIT 20",
    )?;

    let results = stmt
        .query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .filter_map(log_and_discard)
        .collect();

    Ok(results)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Diff analysis
// ═══════════════════════════════════════════════════════════════════════════════

/// Map changed files to affected symbols in the database
/// Returns (symbol_name, symbol_type, file_path)
pub fn map_files_to_symbols_sync(
    conn: &Connection,
    project_id: Option<i64>,
    changed_files: &[String],
) -> Vec<(String, String, String)> {
    let mut symbols = Vec::new();

    for file in changed_files {
        // Escape LIKE wildcards in file path to prevent injection
        let escaped = file
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let file_pattern = format!("%{}", escaped);
        let sql = match project_id {
            Some(_) => {
                "SELECT name, symbol_type, file_path FROM code_symbols \
                 WHERE project_id = ?1 AND file_path LIKE ?2 ESCAPE '\\' \
                 UNION ALL \
                 SELECT name, symbol_type, file_path FROM code_symbols \
                 WHERE project_id IS NULL AND file_path LIKE ?2 ESCAPE '\\' \
                 ORDER BY start_line"
            }
            None => {
                "SELECT name, symbol_type, file_path FROM code_symbols \
                 WHERE project_id IS NULL AND file_path LIKE ?2 ESCAPE '\\' \
                 ORDER BY start_line"
            }
        };

        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => continue,
        };

        if let Ok(rows) = stmt.query_map(params![project_id, file_pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) {
            for row in rows.filter_map(super::log_and_discard) {
                symbols.push(row);
            }
        }
    }

    symbols
}

// ═══════════════════════════════════════════════════════════════════════════════
// Summaries processor
// ═══════════════════════════════════════════════════════════════════════════════

/// Get projects that have modules needing summaries
///
/// NOTE: After code DB sharding, this cross-DB JOIN only works when both
/// tables are in the same database (tests, pre-sharding). For sharded layout,
/// use `get_project_ids_needing_summaries_sync` on code pool +
/// `get_project_paths_by_ids_sync` on main pool.
pub fn get_projects_with_pending_summaries_sync(
    conn: &Connection,
) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT m.project_id, p.path
         FROM codebase_modules m
         JOIN projects p ON p.id = m.project_id
         WHERE m.purpose IS NULL OR m.purpose = ''
         LIMIT 10",
    )?;

    let results = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map(|r| r.filter_map(log_and_discard).collect())?;

    Ok(results)
}

/// Get project IDs that have modules needing summaries.
/// Run this on the code database pool.
pub fn get_project_ids_needing_summaries_sync(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT project_id FROM codebase_modules
         WHERE (purpose IS NULL OR purpose = '') AND project_id IS NOT NULL
         LIMIT 10",
    )?;
    let ids = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(log_and_discard)
        .collect();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    fn setup_conn_with_project() -> (Connection, i64) {
        let conn = setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/path", Some("test")).unwrap();
        (conn, pid)
    }

    /// Add code schema tables needed by background functions
    fn add_code_tables(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS code_symbols (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                name TEXT NOT NULL,
                symbol_type TEXT NOT NULL,
                start_line INTEGER,
                end_line INTEGER,
                signature TEXT,
                indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS codebase_modules (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                module_id TEXT NOT NULL,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                purpose TEXT,
                exports TEXT,
                depends_on TEXT,
                symbol_count INTEGER DEFAULT 0,
                line_count INTEGER DEFAULT 0,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(project_id, module_id)
            );
            CREATE TABLE IF NOT EXISTS pending_embeddings (
                id INTEGER PRIMARY KEY,
                project_id INTEGER,
                file_path TEXT NOT NULL,
                chunk_content TEXT NOT NULL,
                start_line INTEGER NOT NULL DEFAULT 1,
                status TEXT DEFAULT 'pending',
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .unwrap();
    }

    // ========================================================================
    // Scan info / system observations
    // ========================================================================

    #[test]
    fn test_get_scan_info_not_found() {
        let (conn, pid) = setup_conn_with_project();
        let info = observations::get_observation_info_sync(&conn, pid, "nonexistent_key");
        assert!(info.is_none());
    }

    #[test]
    fn test_insert_system_marker_and_get_scan_info() {
        let (conn, pid) = setup_conn_with_project();

        insert_system_marker_sync(&conn, pid, "last_scan", "abc123", "indexing").unwrap();

        let info = observations::get_observation_info_sync(&conn, pid, "last_scan");
        assert!(info.is_some());
        let (content, _updated_at) = info.unwrap();
        assert_eq!(content, "abc123");
    }

    #[test]
    fn test_insert_system_marker_upsert_updates() {
        let (conn, pid) = setup_conn_with_project();

        insert_system_marker_sync(&conn, pid, "scan_key", "old_value", "scan").unwrap();
        insert_system_marker_sync(&conn, pid, "scan_key", "new_value", "scan").unwrap();

        let info = observations::get_observation_info_sync(&conn, pid, "scan_key").unwrap();
        assert_eq!(info.0, "new_value");
    }

    #[test]
    fn test_memory_key_exists() {
        let (conn, pid) = setup_conn_with_project();

        assert!(!observations::observation_key_exists_sync(
            &conn, pid, "some_key"
        ));

        insert_system_marker_sync(&conn, pid, "some_key", "value", "test").unwrap();

        assert!(observations::observation_key_exists_sync(
            &conn, pid, "some_key"
        ));
    }

    #[test]
    fn test_is_time_older_than() {
        let (conn, _pid) = setup_conn_with_project();

        // A time from yesterday should be older than '-2 days' but not '-1 hour'
        let old_time = "2020-01-01 00:00:00";
        assert!(is_time_older_than_sync(&conn, old_time, "-1 day"));

        let future_time = "2099-12-31 23:59:59";
        assert!(!is_time_older_than_sync(&conn, future_time, "-1 day"));
    }

    // ========================================================================
    // Health scanner
    // ========================================================================

    #[test]
    fn test_mark_health_scanned_clears_flags() {
        let (conn, pid) = setup_conn_with_project();

        // Set up the flags
        insert_system_marker_sync(&conn, pid, "health_scan_needed", "1", "health").unwrap();
        insert_system_marker_sync(&conn, pid, "health_fast_scan_done", "1", "health").unwrap();

        mark_health_scanned_sync(&conn, pid).unwrap();

        assert!(!observations::observation_key_exists_sync(
            &conn,
            pid,
            "health_scan_needed"
        ));
        assert!(!observations::observation_key_exists_sync(
            &conn,
            pid,
            "health_fast_scan_done"
        ));
        // Scan time marker should exist
        let info = observations::get_observation_info_sync(&conn, pid, "health_scan_time");
        assert!(info.is_some());
    }

    #[test]
    fn test_clear_old_health_issues() {
        let (conn, pid) = setup_conn_with_project();

        // Insert a health observation
        conn.execute(
            "INSERT INTO system_observations (project_id, key, content, observation_type, category, confidence, source, scope, created_at, updated_at)
             VALUES (?, 'health_issue_1', 'Large function detected', 'health', 'complexity', 0.8, 'code_health', 'project', datetime('now'), datetime('now'))",
            [pid],
        )
        .unwrap();

        clear_old_health_issues_sync(&conn, pid).unwrap();

        // Verify health observations are cleared
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM system_observations WHERE project_id = ? AND observation_type = 'health'",
                [pid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_clear_health_issues_by_categories() {
        let (conn, pid) = setup_conn_with_project();

        // Insert health observations in different categories
        for (key, cat) in &[("h1", "complexity"), ("h2", "dead_code"), ("h3", "other")] {
            conn.execute(
                "INSERT INTO system_observations (project_id, key, content, observation_type, category, confidence, source, scope, created_at, updated_at)
                 VALUES (?, ?, 'issue', 'health', ?, 0.8, 'test', 'project', datetime('now'), datetime('now'))",
                params![pid, key, cat],
            )
            .unwrap();
        }

        observations::delete_observations_by_categories_sync(
            &conn,
            pid,
            "health",
            &["complexity", "dead_code"],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM system_observations WHERE project_id = ? AND observation_type = 'health'",
                [pid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1); // only "other" remains
    }

    // ========================================================================
    // Code symbols queries (require code tables)
    // ========================================================================

    #[test]
    fn test_get_symbols_for_file() {
        let (conn, pid) = setup_conn_with_project();
        add_code_tables(&conn);

        conn.execute(
            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line, signature)
             VALUES (?1, 'src/lib.rs', 'main', 'function', 1, 10, 'fn main()')",
            params![pid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line, signature)
             VALUES (?1, 'src/lib.rs', 'Config', 'struct', 15, 25, 'pub struct Config')",
            params![pid],
        )
        .unwrap();

        let symbols = get_symbols_for_file_sync(&conn, pid, "src/lib.rs").unwrap();
        assert_eq!(symbols.len(), 2);

        // Ordered by name
        assert_eq!(symbols[0].1, "Config");
        assert_eq!(symbols[1].1, "main");
    }

    #[test]
    fn test_delete_pending_embedding() {
        let (conn, pid) = setup_conn_with_project();
        add_code_tables(&conn);

        conn.execute(
            "INSERT INTO pending_embeddings (project_id, file_path, chunk_content, start_line)
             VALUES (?1, 'src/main.rs', 'fn main() {}', 1)",
            params![pid],
        )
        .unwrap();
        let id = conn.last_insert_rowid();

        delete_pending_embedding_sync(&conn, id).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_embeddings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}
