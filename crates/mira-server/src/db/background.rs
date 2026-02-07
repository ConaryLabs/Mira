// crates/mira-server/src/db/background.rs
// Database operations for background workers (capabilities, health, documentation)

use rusqlite::{Connection, params};

use super::log_and_discard;

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
        "SELECT datetime(?1) < datetime('now', ?2)",
        params![time, duration],
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

/// Get large functions for complexity analysis
pub fn get_large_functions_sync(
    conn: &Connection,
    project_id: i64,
    min_lines: i64,
) -> rusqlite::Result<Vec<(String, String, i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT name, file_path, start_line, end_line
         FROM code_symbols
         WHERE project_id = ?
           AND symbol_type = 'function'
           AND end_line IS NOT NULL
           AND (end_line - start_line) >= ?
           AND file_path NOT LIKE '%/tests/%'
           AND file_path NOT LIKE '%_test.rs'
           AND name NOT LIKE 'test_%'
         ORDER BY (end_line - start_line) DESC
         LIMIT 10",
    )?;

    let results = stmt
        .query_map(params![project_id, min_lines], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(log_and_discard)
        .collect();

    Ok(results)
}

/// Get functions with error handling for quality analysis
pub fn get_error_heavy_functions_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<(String, String, i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT name, file_path, start_line, end_line
         FROM code_symbols
         WHERE project_id = ?
           AND symbol_type = 'function'
           AND end_line IS NOT NULL
           AND (end_line - start_line) >= 20
           AND file_path NOT LIKE '%/tests/%'
           AND name NOT LIKE 'test_%'
         ORDER BY (end_line - start_line) DESC
         LIMIT 50",
    )?;

    let results = stmt
        .query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(log_and_discard)
        .collect();

    Ok(results)
}

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
        let mut stmt = match conn.prepare(
            "SELECT name, symbol_type, file_path FROM code_symbols
             WHERE (project_id = ? OR project_id IS NULL) AND file_path LIKE ?
             ORDER BY start_line",
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let file_pattern = format!("%{}", file);
        if let Ok(rows) = stmt.query_map(params![project_id, file_pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) {
            for row in rows.flatten() {
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

// ═══════════════════════════════════════════════════════════════════════════════
// Permission hooks
// ═══════════════════════════════════════════════════════════════════════════════

/// Get permission rules for a tool
pub fn get_permission_rules_sync(conn: &Connection, tool_name: &str) -> Vec<(String, String)> {
    let mut stmt = match conn
        .prepare("SELECT pattern, match_type FROM permission_rules WHERE tool_name = ?")
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map([tool_name], |row| Ok((row.get(0)?, row.get(1)?)))
        .ok()
        .map(|rows| rows.filter_map(log_and_discard).collect())
        .unwrap_or_default()
}
