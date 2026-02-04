// crates/mira-server/src/db/search.rs
// Database operations for code search
//
// Consolidates search-related SQL from:
// - search/crossref.rs (find_callers, find_callees)
// - search/context.rs (symbol bounds lookup)
// - search/keyword.rs (FTS and LIKE searches)

use rusqlite::{Connection, params};

/// Result from a cross-reference query (caller/callee lookup)
#[derive(Debug, Clone)]
pub struct CrossRefResult {
    pub symbol_name: String,
    pub file_path: String,
    pub call_count: i32,
}

/// Find functions that call the given function (by callee_name in call_graph)
pub fn find_callers_sync(
    conn: &Connection,
    target_name: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Vec<CrossRefResult> {
    conn.prepare(
        "SELECT cs.name, cs.file_path, cg.call_count
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cg.callee_name = ?1 AND (?2 IS NULL OR cs.project_id = ?2)
         ORDER BY cg.call_count DESC
         LIMIT ?3",
    )
    .and_then(|mut stmt| {
        stmt.query_map(params![target_name, project_id, limit as i64], |row| {
            Ok(CrossRefResult {
                symbol_name: row.get(0)?,
                file_path: row.get(1)?,
                call_count: row.get::<_, i32>(2).unwrap_or(1),
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
    })
    .unwrap_or_default()
}

/// Find functions that are called by the given function
pub fn find_callees_sync(
    conn: &Connection,
    caller_name: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Vec<CrossRefResult> {
    conn.prepare(
        "SELECT cg.callee_name, cs.file_path, COUNT(*) as cnt
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cs.name = ?1 AND (?2 IS NULL OR cs.project_id = ?2)
         GROUP BY cg.callee_name
         ORDER BY cnt DESC
         LIMIT ?3",
    )
    .and_then(|mut stmt| {
        stmt.query_map(params![caller_name, project_id, limit as i64], |row| {
            Ok(CrossRefResult {
                symbol_name: row.get(0)?,
                file_path: row.get(1)?,
                call_count: row.get::<_, i32>(2).unwrap_or(1),
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
    })
    .unwrap_or_default()
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

/// Full-text search using FTS5
pub fn fts_search_sync(
    conn: &Connection,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Vec<FtsSearchResult> {
    // Prepare query for FTS (quote special chars)
    let fts_query = query
        .split_whitespace()
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ");

    conn.prepare(
        "SELECT c.file_path, c.chunk_content, bm25(code_fts, 1.0, 2.0) as score, c.start_line
         FROM code_fts f
         JOIN code_chunks c ON c.rowid = f.rowid
         WHERE code_fts MATCH ?1 AND (?2 IS NULL OR c.project_id = ?2)
         ORDER BY bm25(code_fts, 1.0, 2.0)
         LIMIT ?3",
    )
    .and_then(|mut stmt| {
        stmt.query_map(params![fts_query, project_id, limit as i64], |row| {
            Ok(FtsSearchResult {
                file_path: row.get(0)?,
                chunk_content: row.get(1)?,
                score: row.get(2)?,
                start_line: row.get(3)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
    })
    .unwrap_or_default()
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
        if let Ok(mut stmt) = conn.prepare_cached(sql) {
            if let Ok(rows) = stmt.query_map(params![project_id, pattern, limit as i64], &map_row) {
                for row in rows.filter_map(|r| r.ok()) {
                    if results.len() >= limit {
                        break;
                    }
                    results.push(row);
                }
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
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}
