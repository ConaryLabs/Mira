// crates/mira-server/src/db/search.rs
// Database operations for code search
//
// Consolidates search-related SQL from:
// - search/crossref.rs (find_callers, find_callees)
// - search/context.rs (symbol bounds lookup)
// - search/keyword.rs (FTS and LIKE searches)

use rusqlite::{Connection, params};

use super::log_and_discard;

/// Result from a cross-reference query (caller/callee lookup)
#[derive(Debug, Clone)]
pub struct CrossRefResult {
    pub symbol_name: String,
    pub file_path: String,
    pub call_count: i32,
    /// Line where the symbol starts (from code_symbols.start_line)
    pub line: Option<i64>,
}

/// Find functions that call the given function (by callee_name in call_graph)
pub fn find_callers_sync(
    conn: &Connection,
    target_name: &str,
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<CrossRefResult>> {
    let mut stmt = conn.prepare(
        "SELECT cs.name, cs.file_path, cg.call_count, cs.start_line
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cg.callee_name = ?1 AND (?2 IS NULL OR cs.project_id = ?2)
         ORDER BY cg.call_count DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![target_name, project_id, limit as i64], |row| {
        Ok(CrossRefResult {
            symbol_name: row.get(0)?,
            file_path: row.get(1)?,
            call_count: row.get::<_, i32>(2).unwrap_or(1),
            line: row.get::<_, Option<i64>>(3)?,
        })
    })?;
    Ok(rows.filter_map(log_and_discard).collect())
}

/// Find functions that are called by the given function
pub fn find_callees_sync(
    conn: &Connection,
    caller_name: &str,
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<CrossRefResult>> {
    let mut stmt = conn.prepare(
        "SELECT cg.callee_name, COALESCE(cs2.file_path, cs.file_path), COUNT(*) as cnt,
                MIN(cs2.start_line) as callee_line
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         LEFT JOIN code_symbols cs2 ON cs2.name = cg.callee_name
             AND (?2 IS NULL OR cs2.project_id = ?2)
         WHERE cs.name = ?1 AND (?2 IS NULL OR cs.project_id = ?2)
         GROUP BY cg.callee_name
         ORDER BY cnt DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![caller_name, project_id, limit as i64], |row| {
        Ok(CrossRefResult {
            symbol_name: row.get(0)?,
            file_path: row.get(1)?,
            call_count: row.get::<_, i32>(2).unwrap_or(1),
            line: row.get::<_, Option<i64>>(3)?,
        })
    })?;
    Ok(rows.filter_map(log_and_discard).collect())
}

/// Get the start and end line of a symbol
pub fn get_symbol_bounds_sync(
    conn: &Connection,
    file_path: &str,
    symbol_name: &str,
    project_id: Option<i64>,
) -> Option<(u32, u32)> {
    conn.query_row(
        "SELECT start_line, end_line FROM code_symbols
         WHERE (?1 IS NULL OR project_id = ?1) AND file_path = ?2 AND name = ?3
         LIMIT 1",
        params![project_id, file_path, symbol_name],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .ok()
}

/// Result from FTS search
#[derive(Debug, Clone)]
pub struct FtsSearchResult {
    pub file_path: String,
    pub chunk_content: String,
    pub score: f64,
    pub start_line: Option<i64>,
}

/// Search the FTS5 index with a pre-built query expression.
///
/// # Safety (logical)
/// The `query` parameter is passed directly to FTS5 MATCH. Callers MUST
/// ensure the query is constructed from escaped terms (via `escape_fts_term`)
/// or is a validated FTS5 expression. Passing raw user input may cause
/// unexpected query behavior.
pub fn fts_search_sync(
    conn: &Connection,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Vec<FtsSearchResult> {
    conn.prepare(
        "SELECT c.file_path, c.chunk_content, bm25(code_fts, 1.0, 2.0) as score, c.start_line
         FROM code_fts f
         JOIN code_chunks c ON c.rowid = f.rowid
         WHERE code_fts MATCH ?1 AND (?2 IS NULL OR c.project_id = ?2)
         ORDER BY bm25(code_fts, 1.0, 2.0)
         LIMIT ?3",
    )
    .and_then(|mut stmt| {
        stmt.query_map(params![query, project_id, limit as i64], |row| {
            Ok(FtsSearchResult {
                file_path: row.get(0)?,
                chunk_content: row.get(1)?,
                score: row.get(2)?,
                start_line: row.get(3)?,
            })
        })
        .map(|rows| rows.filter_map(log_and_discard).collect())
    })
    .unwrap_or_else(|e| {
        tracing::warn!(
            "fts_search_sync query failed (possible FTS index corruption): {}",
            e
        );
        Vec::new()
    })
}

/// Result from chunk LIKE search
#[derive(Debug, Clone)]
pub struct ChunkSearchResult {
    pub file_path: String,
    pub chunk_content: String,
    pub start_line: Option<i64>,
}

/// Generic LIKE search: iterates patterns, accumulates rows up to `limit`.
fn like_search_sync<T, F>(
    conn: &Connection,
    sql: &str,
    patterns: &[String],
    project_id: i64,
    limit: usize,
    map_row: F,
) -> Vec<T>
where
    F: Fn(&rusqlite::Row) -> rusqlite::Result<T>,
{
    let mut results = Vec::new();
    for pattern in patterns {
        if let Ok(mut stmt) = conn.prepare_cached(sql)
            && let Ok(rows) = stmt.query_map(params![project_id, pattern, limit as i64], &map_row)
        {
            for row in rows.filter_map(log_and_discard) {
                if results.len() >= limit {
                    break;
                }
                results.push(row);
            }
        }
        if results.len() >= limit {
            break;
        }
    }
    results
}

/// Search code chunks using LIKE patterns
pub fn chunk_like_search_sync(
    conn: &Connection,
    patterns: &[String],
    project_id: i64,
    limit: usize,
) -> Vec<ChunkSearchResult> {
    like_search_sync(
        conn,
        "SELECT file_path, chunk_content, start_line FROM code_chunks
         WHERE project_id = ? AND LOWER(chunk_content) LIKE ?
         LIMIT ?",
        patterns,
        project_id,
        limit,
        |row| {
            Ok(ChunkSearchResult {
                file_path: row.get(0)?,
                chunk_content: row.get(1)?,
                start_line: row.get(2)?,
            })
        },
    )
}

/// Result from symbol LIKE search
#[derive(Debug, Clone)]
pub struct SymbolSearchResult {
    pub file_path: String,
    pub name: String,
    pub signature: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
}

/// Search symbols using LIKE patterns
pub fn symbol_like_search_sync(
    conn: &Connection,
    patterns: &[String],
    project_id: i64,
    limit: usize,
) -> Vec<SymbolSearchResult> {
    like_search_sync(
        conn,
        "SELECT file_path, name, signature, start_line, end_line
         FROM code_symbols
         WHERE project_id = ? AND LOWER(name) LIKE ?
         LIMIT ?",
        patterns,
        project_id,
        limit,
        |row| {
            Ok(SymbolSearchResult {
                file_path: row.get(0)?,
                name: row.get(1)?,
                signature: row.get(2)?,
                start_line: row.get(3)?,
                end_line: row.get(4)?,
            })
        },
    )
}

/// Result from semantic code search
#[derive(Debug, Clone)]
pub struct SemanticCodeResult {
    pub file_path: String,
    pub chunk_content: String,
    pub distance: f32,
    pub start_line: i64,
}

/// Semantic code search using vector similarity
pub fn semantic_code_search_sync(
    conn: &Connection,
    embedding_bytes: &[u8],
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<SemanticCodeResult>> {
    let mut stmt = conn.prepare(
        "SELECT file_path, chunk_content, vec_distance_cosine(embedding, ?2) as distance, start_line
         FROM vec_code
         WHERE project_id = ?1 OR ?1 IS NULL
         ORDER BY distance
         LIMIT ?3",
    )?;

    let results = stmt
        .query_map(params![project_id, embedding_bytes, limit as i64], |row| {
            Ok(SemanticCodeResult {
                file_path: row.get(0)?,
                chunk_content: row.get(1)?,
                distance: row.get(2)?,
                start_line: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
            })
        })?
        .filter_map(log_and_discard)
        .collect();

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::{seed_call_edge, seed_symbol, setup_test_connection};

    /// Set up test connection with code schema tables
    fn setup_conn_with_code_schema() -> (Connection, i64) {
        let conn = setup_test_connection();
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
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON code_symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_file ON code_symbols(project_id, file_path);
            CREATE TABLE IF NOT EXISTS call_graph (
                id INTEGER PRIMARY KEY,
                caller_id INTEGER REFERENCES code_symbols(id),
                callee_name TEXT NOT NULL,
                callee_id INTEGER REFERENCES code_symbols(id),
                call_count INTEGER DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_calls_caller ON call_graph(caller_id);
            CREATE INDEX IF NOT EXISTS idx_calls_callee_name ON call_graph(callee_name, call_count DESC);
            CREATE TABLE IF NOT EXISTS code_chunks (
                id INTEGER PRIMARY KEY,
                project_id INTEGER,
                file_path TEXT NOT NULL,
                chunk_content TEXT NOT NULL,
                start_line INTEGER NOT NULL DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_code_chunks_project ON code_chunks(project_id);
            CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(
                file_path,
                chunk_content,
                project_id UNINDEXED,
                start_line UNINDEXED,
                content='',
                tokenize=\"unicode61 remove_diacritics 1 tokenchars '_'\"
            );",
        )
        .unwrap();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();
        (conn, pid)
    }

    // ========================================================================
    // find_callers_sync / find_callees_sync
    // ========================================================================

    #[test]
    fn test_find_callers_single_caller() {
        let (conn, pid) = setup_conn_with_code_schema();

        let caller_id = seed_symbol(&conn, pid, "handler", "src/api.rs", "function", 1, 10);
        seed_call_edge(&conn, caller_id, "process_request");

        let results = find_callers_sync(&conn, "process_request", Some(pid), 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol_name, "handler");
        assert_eq!(results[0].file_path, "src/api.rs");
    }

    #[test]
    fn test_find_callers_multiple() {
        let (conn, pid) = setup_conn_with_code_schema();

        let a = seed_symbol(&conn, pid, "func_a", "src/a.rs", "function", 1, 5);
        let b = seed_symbol(&conn, pid, "func_b", "src/b.rs", "function", 1, 5);
        seed_call_edge(&conn, a, "target");
        seed_call_edge(&conn, b, "target");

        let results = find_callers_sync(&conn, "target", Some(pid), 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_find_callers_no_results() {
        let (conn, pid) = setup_conn_with_code_schema();

        let results = find_callers_sync(&conn, "nonexistent_fn", Some(pid), 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_find_callees_single() {
        let (conn, pid) = setup_conn_with_code_schema();

        let main_id = seed_symbol(&conn, pid, "main", "src/main.rs", "function", 1, 20);
        seed_call_edge(&conn, main_id, "init");

        let results = find_callees_sync(&conn, "main", Some(pid), 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol_name, "init");
    }

    #[test]
    fn test_find_callees_multiple() {
        let (conn, pid) = setup_conn_with_code_schema();

        let main_id = seed_symbol(&conn, pid, "main", "src/main.rs", "function", 1, 30);
        seed_call_edge(&conn, main_id, "init");
        seed_call_edge(&conn, main_id, "run_server");
        seed_call_edge(&conn, main_id, "shutdown");

        let results = find_callees_sync(&conn, "main", Some(pid), 10).unwrap();
        assert_eq!(results.len(), 3);
    }

    // ========================================================================
    // get_symbol_bounds_sync
    // ========================================================================

    #[test]
    fn test_get_symbol_bounds_found() {
        let (conn, pid) = setup_conn_with_code_schema();

        seed_symbol(&conn, pid, "my_func", "src/lib.rs", "function", 10, 25);

        let bounds = get_symbol_bounds_sync(&conn, "src/lib.rs", "my_func", Some(pid));
        assert_eq!(bounds, Some((10, 25)));
    }

    #[test]
    fn test_get_symbol_bounds_not_found() {
        let (conn, pid) = setup_conn_with_code_schema();

        let bounds = get_symbol_bounds_sync(&conn, "src/lib.rs", "nonexistent", Some(pid));
        assert!(bounds.is_none());
    }

    // ========================================================================
    // FTS search (requires seeded code_fts and code_chunks)
    // ========================================================================

    #[test]
    fn test_fts_search_with_seeded_data() {
        let (conn, pid) = setup_conn_with_code_schema();

        // Seed code_chunks
        conn.execute(
            "INSERT INTO code_chunks (project_id, file_path, chunk_content, start_line)
             VALUES (?1, 'src/db/pool.rs', 'fn open_database() { connection pool setup }', 1)",
            params![pid],
        )
        .unwrap();

        // Seed code_fts to match
        conn.execute(
            "INSERT INTO code_fts (rowid, file_path, chunk_content, project_id, start_line)
             VALUES (last_insert_rowid(), 'src/db/pool.rs', 'fn open_database() { connection pool setup }', ?1, 1)",
            params![pid],
        )
        .unwrap();

        let results = fts_search_sync(&conn, "open_database", Some(pid), 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "src/db/pool.rs");
        assert!(results[0].chunk_content.contains("open_database"));
    }

    #[test]
    fn test_fts_search_no_results() {
        let (conn, pid) = setup_conn_with_code_schema();

        let results = fts_search_sync(&conn, "completely_nonexistent_term", Some(pid), 10);
        assert!(results.is_empty());
    }

    // ========================================================================
    // chunk_like_search_sync
    // ========================================================================

    #[test]
    fn test_chunk_like_search() {
        let (conn, pid) = setup_conn_with_code_schema();

        conn.execute(
            "INSERT INTO code_chunks (project_id, file_path, chunk_content, start_line)
             VALUES (?1, 'src/config.rs', 'pub struct DatabaseConfig { host: String }', 5)",
            params![pid],
        )
        .unwrap();

        let patterns = vec!["%databaseconfig%".to_string()];
        let results = chunk_like_search_sync(&conn, &patterns, pid, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "src/config.rs");
        assert_eq!(results[0].start_line, Some(5));
    }

    // ========================================================================
    // symbol_like_search_sync
    // ========================================================================

    #[test]
    fn test_symbol_like_search() {
        let (conn, pid) = setup_conn_with_code_schema();

        seed_symbol(
            &conn,
            pid,
            "DatabasePool",
            "src/db/pool.rs",
            "struct",
            1,
            50,
        );
        seed_symbol(
            &conn,
            pid,
            "open_connection",
            "src/db/pool.rs",
            "function",
            55,
            80,
        );

        let patterns = vec!["%database%".to_string()];
        let results = symbol_like_search_sync(&conn, &patterns, pid, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "DatabasePool");
        assert_eq!(results[0].file_path, "src/db/pool.rs");
    }

    #[test]
    fn test_symbol_like_search_multiple_patterns() {
        let (conn, pid) = setup_conn_with_code_schema();

        seed_symbol(
            &conn,
            pid,
            "DatabasePool",
            "src/db/pool.rs",
            "struct",
            1,
            50,
        );
        seed_symbol(
            &conn,
            pid,
            "open_connection",
            "src/db/conn.rs",
            "function",
            1,
            30,
        );

        let patterns = vec!["%database%".to_string(), "%connection%".to_string()];
        let results = symbol_like_search_sync(&conn, &patterns, pid, 10);
        assert_eq!(results.len(), 2);
    }
}
