// crates/mira-server/src/search/keyword.rs
// FTS5-powered keyword search for code

use crate::db::{chunk_like_search_sync, fts_search_sync, symbol_like_search_sync};
use rusqlite::Connection;
use std::path::Path;

/// Result from keyword search: (file_path, content, score, start_line)
pub type KeywordResult = (String, String, f32, i64);

/// FTS5-powered keyword search
/// Uses SQLite full-text search for fast, accurate keyword matching
pub fn keyword_search(
    conn: &Connection,
    query: &str,
    project_id: Option<i64>,
    project_path: Option<&str>,
    limit: usize,
) -> Vec<KeywordResult> {
    // Clean query for FTS5 - escape special characters and build search terms
    let fts_query = build_fts_query(query);
    if fts_query.is_empty() {
        return Vec::new();
    }

    // Try FTS5 search first
    let fts_results = fts5_search(conn, &fts_query, project_id, limit);
    if !fts_results.is_empty() {
        return fts_results;
    }

    // Fallback to LIKE search if FTS5 fails or returns no results
    // This handles edge cases and ensures we always return something if possible
    like_search(conn, query, project_id, project_path, limit)
}

/// Build FTS5 query from user input
/// Handles special characters and builds proper FTS5 syntax
fn build_fts_query(query: &str) -> String {
    // Split into terms
    let terms: Vec<&str> = query.split_whitespace().filter(|t| !t.is_empty()).collect();

    if terms.is_empty() {
        return String::new();
    }

    // For single terms, use prefix matching
    if terms.len() == 1 {
        let term = terms[0];
        // Escape special FTS5 characters and add prefix match
        let cleaned = escape_fts_term(term);
        if cleaned.is_empty() {
            return String::new();
        }
        return format!("{}*", cleaned);
    }

    // For multiple terms, use OR matching with prefix on last term
    let mut query_parts: Vec<String> = Vec::new();
    for (i, term) in terms.iter().enumerate() {
        let cleaned = escape_fts_term(term);
        if cleaned.is_empty() {
            continue;
        }
        if i == terms.len() - 1 {
            // Prefix match on last term for partial matching
            query_parts.push(format!("{}*", cleaned));
        } else {
            query_parts.push(cleaned);
        }
    }

    query_parts.join(" OR ")
}

/// Escape special FTS5 characters
fn escape_fts_term(term: &str) -> String {
    // FTS5 special characters: " - * ( ) ^
    // Remove or escape them for safe querying
    term.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// FTS5 full-text search
fn fts5_search(
    conn: &Connection,
    fts_query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Vec<KeywordResult> {
    fts_search_sync(conn, fts_query, project_id, limit)
        .into_iter()
        .map(|r| {
            // Convert BM25 score to 0-1 range (BM25 is negative, lower is better)
            // Typical range is -20 to 0, so we normalize
            let score = ((-r.score + 20.0) / 20.0).clamp(0.0, 1.0) as f32;
            (
                r.file_path,
                r.chunk_content,
                score,
                r.start_line.unwrap_or(0),
            )
        })
        .collect()
}

/// Fallback LIKE-based search (when FTS5 fails or for edge cases)
fn like_search(
    conn: &Connection,
    query: &str,
    project_id: Option<i64>,
    project_path: Option<&str>,
    limit: usize,
) -> Vec<KeywordResult> {
    let mut results = Vec::new();

    // Split query into terms for flexible matching
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return results;
    }

    // Build LIKE patterns
    let like_patterns: Vec<String> = terms
        .iter()
        .map(|t| format!("%{}%", t.to_lowercase()))
        .collect();

    // Search vec_code chunk_content
    if let Some(pid) = project_id {
        let chunk_results = chunk_like_search_sync(conn, &like_patterns, pid, limit);
        for chunk in chunk_results {
            let start_line = chunk.start_line.unwrap_or(0);
            // Avoid duplicates
            if !results
                .iter()
                .any(|(f, c, _, _)| f == &chunk.file_path && c == &chunk.chunk_content)
            {
                results.push((chunk.file_path, chunk.chunk_content, 0.5, start_line));
            }
            if results.len() >= limit {
                break;
            }
        }
    }

    // Also search symbol names for direct matches
    if let Some(pid) = project_id {
        let symbol_results = symbol_like_search_sync(conn, &like_patterns, pid, limit);
        for sym in symbol_results {
            // Try to read the actual code from file
            let content = if let Some(proj_path) = project_path {
                let full_path = Path::new(proj_path).join(&sym.file_path);
                if let Ok(file_content) = std::fs::read_to_string(&full_path) {
                    let lines: Vec<&str> = file_content.lines().collect();
                    let start_idx = (sym.start_line as usize).saturating_sub(1).min(lines.len());
                    let end_idx = (sym.end_line as usize).min(lines.len());
                    if start_idx < end_idx {
                        lines[start_idx..end_idx].join("\n")
                    } else {
                        sym.signature.clone().unwrap_or_else(|| sym.name.clone())
                    }
                } else {
                    sym.signature.clone().unwrap_or_else(|| sym.name.clone())
                }
            } else {
                sym.signature.clone().unwrap_or_else(|| sym.name.clone())
            };

            // Avoid duplicates
            if !results
                .iter()
                .any(|(f, _, _, _)| f == &sym.file_path && content.contains(&sym.name))
            {
                results.push((sym.file_path, content, 0.6, sym.start_line));
            }
            if results.len() >= limit {
                break;
            }
        }
    }

    results.truncate(limit);
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // escape_fts_term tests
    // ============================================================================

    #[test]
    fn test_escape_fts_term_alphanumeric() {
        assert_eq!(escape_fts_term("hello"), "hello");
        assert_eq!(escape_fts_term("hello123"), "hello123");
        assert_eq!(escape_fts_term("test_name"), "test_name");
    }

    #[test]
    fn test_escape_fts_term_special_chars() {
        assert_eq!(escape_fts_term("hello*world"), "helloworld");
        assert_eq!(escape_fts_term("test-case"), "testcase");
        assert_eq!(escape_fts_term("fn()"), "fn");
        assert_eq!(escape_fts_term("a^b"), "ab");
        assert_eq!(escape_fts_term("\"quoted\""), "quoted");
    }

    #[test]
    fn test_escape_fts_term_all_special() {
        assert_eq!(escape_fts_term("*-()^\""), "");
    }

    #[test]
    fn test_escape_fts_term_mixed() {
        assert_eq!(escape_fts_term("fn main()"), "fnmain");
        assert_eq!(escape_fts_term("user_id = 123"), "user_id123");
    }

    // ============================================================================
    // build_fts_query tests
    // ============================================================================

    #[test]
    fn test_build_fts_query_empty() {
        assert_eq!(build_fts_query(""), "");
        assert_eq!(build_fts_query("   "), "");
    }

    #[test]
    fn test_build_fts_query_single_term() {
        assert_eq!(build_fts_query("search"), "search*");
        assert_eq!(build_fts_query("Database"), "Database*");
    }

    #[test]
    fn test_build_fts_query_single_term_with_special() {
        assert_eq!(build_fts_query("fn()"), "fn*");
        assert_eq!(build_fts_query("*test*"), "test*");
    }

    #[test]
    fn test_build_fts_query_multiple_terms() {
        assert_eq!(build_fts_query("search code"), "search OR code*");
        assert_eq!(build_fts_query("find user data"), "find OR user OR data*");
    }

    #[test]
    fn test_build_fts_query_multiple_terms_with_special() {
        // Special chars are stripped, but terms remain
        assert_eq!(build_fts_query("fn() main()"), "fn OR main*");
    }

    #[test]
    fn test_build_fts_query_all_special_terms() {
        // If all terms become empty after escaping, return empty
        assert_eq!(build_fts_query("() * -"), "");
    }

    #[test]
    fn test_build_fts_query_partial_special_terms() {
        // Mixed: some valid, some empty after escape
        // "hello" stays, "()" becomes empty, "world" stays
        let result = build_fts_query("hello () world");
        assert!(result.contains("hello"));
        assert!(result.contains("world*"));
    }

    // ============================================================================
    // KeywordResult type tests
    // ============================================================================

    #[test]
    fn test_keyword_result_type() {
        let result: KeywordResult = ("src/main.rs".to_string(), "fn main()".to_string(), 0.85, 10);
        assert_eq!(result.0, "src/main.rs");
        assert_eq!(result.1, "fn main()");
        assert!((result.2 - 0.85).abs() < 0.001);
        assert_eq!(result.3, 10);
    }
}
