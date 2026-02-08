// crates/mira-server/src/search/keyword.rs
// FTS5-powered keyword search for code

use super::utils::{Locatable, deduplicate_by_location};
use crate::db::{
    SymbolSearchResult, chunk_like_search_sync, fts_search_sync, symbol_like_search_sync,
};
use crate::utils::safe_join;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Result from keyword search: (file_path, content, score, start_line)
pub type KeywordResult = (String, String, f32, i64);

impl Locatable for KeywordResult {
    fn file_path(&self) -> &str {
        &self.0
    }
    fn start_line(&self) -> i64 {
        self.3
    }
    fn score(&self) -> f32 {
        self.2
    }
}

/// Score boost for results with proximity matches (20%)
const PROXIMITY_BOOST: f32 = 1.2;

/// FTS5 query plan with strict (AND), relaxed (OR), and proximity (NEAR) variants
#[derive(Debug)]
struct FtsQueryPlan {
    /// AND query — all terms must match (tried first)
    strict: String,
    /// OR query — any term can match (fallback if AND yields nothing)
    relaxed: Option<String>,
    /// NEAR query — terms within 10 tokens of each other (for boosting)
    proximity: Option<String>,
}

impl FtsQueryPlan {
    fn is_empty(&self) -> bool {
        self.strict.is_empty()
    }
}

/// FTS5-powered keyword search with unified scoring and tree-guided scoping
///
/// Runs three search strategies and merges results:
/// 1. FTS5 full-text search (AND-first, OR fallback)
/// 2. Symbol name matching (always runs alongside FTS5)
/// 3. LIKE chunk search (supplements if results are sparse)
///
/// When the cartographer module tree is available, results in relevant
/// module subtrees receive a score boost, pushing them above unrelated matches.
pub fn keyword_search(
    conn: &Connection,
    query: &str,
    project_id: Option<i64>,
    project_path: Option<&str>,
    limit: usize,
) -> Vec<KeywordResult> {
    let plan = build_fts_query(query);
    if plan.is_empty() {
        return Vec::new();
    }

    // Tree-guided scope: score modules to identify relevant subtrees
    let scope_paths = project_id.and_then(|pid| super::tree::narrow_by_modules(conn, query, pid));
    let has_scope = scope_paths.is_some();

    // Over-fetch when scoping so we have enough results after boosting/sorting
    let fetch_limit = if has_scope { limit * 3 } else { limit };

    let mut all_results: Vec<KeywordResult> = Vec::new();

    // Strategy 1: FTS5 search — try AND (strict) first, fall back to OR (relaxed)
    let fts_results = fts5_search(conn, &plan.strict, project_id, fetch_limit);
    if fts_results.is_empty() {
        if let Some(ref relaxed) = plan.relaxed {
            all_results.extend(fts5_search(conn, relaxed, project_id, fetch_limit));
        }
    } else {
        all_results.extend(fts_results);
    }

    // Strategy 2: Symbol name search (always runs, not just as fallback)
    // Cache file reads so multiple symbols from one file don't hit disk repeatedly.
    let mut symbol_file_cache: HashMap<String, Option<String>> = HashMap::new();
    if let Some(pid) = project_id {
        let terms: Vec<&str> = query.split_whitespace().collect();
        if !terms.is_empty() {
            let like_patterns: Vec<String> = terms
                .iter()
                .map(|t| format!("%{}%", strip_like_wildcards(&t.to_lowercase())))
                .collect();
            let symbol_results = symbol_like_search_sync(conn, &like_patterns, pid, fetch_limit);
            for sym in symbol_results {
                let content = read_symbol_content(&sym, project_path, &mut symbol_file_cache);
                let score = score_symbol_match(&sym.name, query);
                all_results.push((sym.file_path, content, score, sym.start_line));
            }
        }
    }

    // Strategy 3: LIKE chunk search (supplement when results are sparse)
    if all_results.len() < fetch_limit
        && let Some(pid) = project_id
    {
        let terms: Vec<&str> = query.split_whitespace().collect();
        if !terms.is_empty() {
            let like_patterns: Vec<String> = terms
                .iter()
                .map(|t| format!("%{}%", strip_like_wildcards(&t.to_lowercase())))
                .collect();
            let remaining = fetch_limit - all_results.len();
            let chunk_results = chunk_like_search_sync(conn, &like_patterns, pid, remaining);
            for chunk in chunk_results {
                let start_line = chunk.start_line.unwrap_or(0);
                all_results.push((chunk.file_path, chunk.chunk_content, 0.4, start_line));
            }
        }
    }

    // Apply tree scope boost: results in relevant module subtrees get higher scores
    if let Some(ref paths) = scope_paths {
        for result in &mut all_results {
            if super::tree::path_in_scope(&result.0, paths) {
                result.2 *= super::tree::SCOPE_BOOST;
            }
        }
    }

    // Apply proximity boost: results where terms appear near each other score higher
    if let Some(ref near_query) = plan.proximity {
        let proximity_hits = fts5_search(conn, near_query, project_id, fetch_limit);
        if !proximity_hits.is_empty() {
            // Build file -> line set for O(1) lookups without per-result string clones.
            let mut near_lines_by_file: HashMap<String, HashSet<i64>> = HashMap::new();
            for (file_path, _content, _score, start_line) in proximity_hits {
                near_lines_by_file
                    .entry(file_path)
                    .or_default()
                    .insert(start_line);
            }
            for result in &mut all_results {
                if near_lines_by_file
                    .get(&result.0)
                    .is_some_and(|lines| lines.contains(&result.3))
                {
                    result.2 *= PROXIMITY_BOOST;
                }
            }
        }
    }

    // Deduplicate by (file_path, start_line), keep highest score, sort, truncate
    dedup_and_sort(all_results, limit)
}

/// Build FTS5 query plan from user input
///
/// For single terms: prefix match (no AND/OR distinction).
/// For multiple terms: AND query (strict) with OR fallback (relaxed).
/// FTS5 implicit AND = space-separated terms. Explicit OR = "term1 OR term2".
fn build_fts_query(query: &str) -> FtsQueryPlan {
    let terms: Vec<&str> = query.split_whitespace().filter(|t| !t.is_empty()).collect();

    if terms.is_empty() {
        return FtsQueryPlan {
            strict: String::new(),
            relaxed: None,
            proximity: None,
        };
    }

    // Clean all terms, adding prefix match on the last one
    let cleaned: Vec<String> = terms
        .iter()
        .enumerate()
        .filter_map(|(i, term)| {
            let c = escape_fts_term(term);
            if c.is_empty() {
                return None;
            }
            if i == terms.len() - 1 {
                Some(format!("{}*", c))
            } else {
                Some(c)
            }
        })
        .collect();

    if cleaned.is_empty() {
        return FtsQueryPlan {
            strict: String::new(),
            relaxed: None,
            proximity: None,
        };
    }

    // Single term — no AND vs OR distinction, no proximity
    if cleaned.len() == 1 {
        return FtsQueryPlan {
            strict: cleaned[0].clone(),
            relaxed: None,
            proximity: None,
        };
    }

    // Build NEAR query for proximity boosting (terms without prefix *)
    // FTS5 NEAR syntax: NEAR(term1 term2, distance)
    let near_terms: Vec<String> = terms
        .iter()
        .filter_map(|term| {
            let c = escape_fts_term(term);
            if c.is_empty() { None } else { Some(c) }
        })
        .collect();
    let proximity = if near_terms.len() >= 2 {
        Some(format!("NEAR({}, 10)", near_terms.join(" ")))
    } else {
        None
    };

    // Multiple terms: space-separated = implicit AND in FTS5
    FtsQueryPlan {
        strict: cleaned.join(" "),
        relaxed: Some(cleaned.join(" OR ")),
        proximity,
    }
}

/// Strip SQL LIKE wildcards (%, _, \) from user-supplied terms to prevent
/// wildcard injection in LIKE patterns. Stripping rather than escaping
/// avoids the need for an `ESCAPE` clause in every downstream SQL query.
fn strip_like_wildcards(term: &str) -> String {
    term.chars()
        .filter(|c| *c != '%' && *c != '_' && *c != '\\')
        .collect()
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

/// Read a symbol's source code from the filesystem, falling back to signature/name
fn read_symbol_content(
    sym: &SymbolSearchResult,
    project_path: Option<&str>,
    file_cache: &mut HashMap<String, Option<String>>,
) -> String {
    if let Some(proj_path) = project_path {
        let cached = file_cache.entry(sym.file_path.clone()).or_insert_with(|| {
            let full_path = safe_join(Path::new(proj_path), &sym.file_path)?;
            std::fs::read_to_string(&full_path).ok()
        });

        if let Some(file_content) = cached.as_deref() {
            let start_idx = (sym.start_line as usize).saturating_sub(1);
            let line_count = (sym.end_line.saturating_sub(sym.start_line) + 1) as usize;
            let snippet: Vec<&str> = file_content
                .lines()
                .skip(start_idx)
                .take(line_count)
                .collect();
            if !snippet.is_empty() {
                return snippet.join("\n");
            }
        }
    }
    sym.signature.clone().unwrap_or_else(|| sym.name.clone())
}

/// Score a symbol name match against the query
///
/// Returns 0.0–1.0 based on match quality:
/// - 0.95: exact match (query maps directly to symbol name)
/// - 0.85: symbol name contains the full query as substring
/// - 0.75: all query terms appear in the symbol name
/// - 0.55–0.70: partial term matches (scaled by fraction matched)
fn score_symbol_match(symbol_name: &str, query: &str) -> f32 {
    let name_lower = symbol_name.to_lowercase();
    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    // Exact match: query with spaces replaced by _ or concatenated matches name
    let query_snake = query_lower.replace(' ', "_");
    let query_concat = query_lower.replace(' ', "");
    if name_lower == query_snake || name_lower == query_concat {
        return 0.95;
    }

    // Substring: symbol name contains the full query form
    if name_lower.contains(&query_snake) || name_lower.contains(&query_concat) {
        return 0.85;
    }

    // Count how many query terms appear in the symbol name
    let matches = query_terms
        .iter()
        .filter(|t| name_lower.contains(**t))
        .count();
    if matches == query_terms.len() && !query_terms.is_empty() {
        return 0.75;
    }

    // Partial: scale by fraction of terms matched
    if matches > 0 {
        return 0.55 + (0.15 * matches as f32 / query_terms.len().max(1) as f32);
    }

    // Matched by LIKE but no direct term overlap (e.g. substring of a term)
    0.50
}

/// Deduplicate results by (file_path, start_line), keep highest score, sort descending
fn dedup_and_sort(results: Vec<KeywordResult>, limit: usize) -> Vec<KeywordResult> {
    let mut deduped = deduplicate_by_location(results);
    deduped.truncate(limit);
    deduped
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
    // build_fts_query tests (now returns FtsQueryPlan)
    // ============================================================================

    #[test]
    fn test_build_fts_query_empty() {
        let plan = build_fts_query("");
        assert!(plan.is_empty());
        let plan = build_fts_query("   ");
        assert!(plan.is_empty());
    }

    #[test]
    fn test_build_fts_query_single_term() {
        let plan = build_fts_query("search");
        assert_eq!(plan.strict, "search*");
        assert!(plan.relaxed.is_none());
        assert!(plan.proximity.is_none()); // single term has no proximity

        let plan = build_fts_query("Database");
        assert_eq!(plan.strict, "Database*");
        assert!(plan.relaxed.is_none());
        assert!(plan.proximity.is_none());
    }

    #[test]
    fn test_build_fts_query_single_term_with_special() {
        let plan = build_fts_query("fn()");
        assert_eq!(plan.strict, "fn*");
        assert!(plan.relaxed.is_none());
        assert!(plan.proximity.is_none());

        let plan = build_fts_query("*test*");
        assert_eq!(plan.strict, "test*");
        assert!(plan.relaxed.is_none());
        assert!(plan.proximity.is_none());
    }

    #[test]
    fn test_build_fts_query_multiple_terms_and_first() {
        // Multiple terms: strict = AND (space-separated), relaxed = OR
        let plan = build_fts_query("search code");
        assert_eq!(plan.strict, "search code*");
        assert_eq!(plan.relaxed.as_deref(), Some("search OR code*"));
        assert_eq!(plan.proximity.as_deref(), Some("NEAR(search code, 10)"));

        let plan = build_fts_query("find user data");
        assert_eq!(plan.strict, "find user data*");
        assert_eq!(plan.relaxed.as_deref(), Some("find OR user OR data*"));
        assert_eq!(plan.proximity.as_deref(), Some("NEAR(find user data, 10)"));
    }

    #[test]
    fn test_build_fts_query_multiple_terms_with_special() {
        let plan = build_fts_query("fn() main()");
        assert_eq!(plan.strict, "fn main*");
        assert_eq!(plan.relaxed.as_deref(), Some("fn OR main*"));
        assert_eq!(plan.proximity.as_deref(), Some("NEAR(fn main, 10)"));
    }

    #[test]
    fn test_build_fts_query_all_special_terms() {
        let plan = build_fts_query("() * -");
        assert!(plan.is_empty());
    }

    #[test]
    fn test_build_fts_query_partial_special_terms() {
        let plan = build_fts_query("hello () world");
        assert!(plan.strict.contains("hello"));
        assert!(plan.strict.contains("world*"));
        // strict should be AND (space-separated)
        assert!(!plan.strict.contains("OR"));
        // relaxed should be OR
        let relaxed = plan.relaxed.unwrap();
        assert!(relaxed.contains("hello"));
        assert!(relaxed.contains("OR"));
        assert!(relaxed.contains("world*"));
        // proximity should use clean terms without prefix *
        let proximity = plan.proximity.unwrap();
        assert_eq!(proximity, "NEAR(hello world, 10)");
    }

    // ============================================================================
    // score_symbol_match tests
    // ============================================================================

    #[test]
    fn test_score_symbol_exact_match() {
        // "database pool" -> "database_pool" (snake_case exact)
        assert!((score_symbol_match("database_pool", "database pool") - 0.95).abs() < 0.01);
        // "databasepool" (concatenated exact)
        assert!((score_symbol_match("databasepool", "database pool") - 0.95).abs() < 0.01);
    }

    #[test]
    fn test_score_symbol_substring_match() {
        // Symbol contains the full query form
        assert!((score_symbol_match("get_database_pool", "database pool") - 0.85).abs() < 0.01);
        assert!(
            (score_symbol_match("create_database_pool_sync", "database pool") - 0.85).abs() < 0.01
        );
    }

    #[test]
    fn test_score_symbol_all_terms_match() {
        // All terms present but not as a contiguous substring
        assert!((score_symbol_match("pool_for_database", "database pool") - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_score_symbol_partial_match() {
        // Only some terms match
        let score = score_symbol_match("database_connection", "database pool");
        assert!(score > 0.55 && score < 0.75);
    }

    #[test]
    fn test_score_symbol_no_term_overlap() {
        assert!((score_symbol_match("xyz_handler", "abc") - 0.50).abs() < 0.01);
    }

    #[test]
    fn test_score_symbol_single_term() {
        // Single-word query, exact match
        assert!((score_symbol_match("search", "search") - 0.95).abs() < 0.01);
        // Single-word query, substring
        assert!((score_symbol_match("keyword_search", "search") - 0.85).abs() < 0.01);
    }

    // ============================================================================
    // dedup_and_sort tests
    // ============================================================================

    #[test]
    fn test_dedup_keeps_highest_score() {
        let results = vec![
            ("src/a.rs".into(), "fn a()".into(), 0.5, 10),
            ("src/a.rs".into(), "fn a() { body }".into(), 0.9, 10), // same file+line, higher score
        ];
        let deduped = dedup_and_sort(results, 10);
        assert_eq!(deduped.len(), 1);
        assert!((deduped[0].2 - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_dedup_different_lines_kept() {
        let results = vec![
            ("src/a.rs".into(), "fn a()".into(), 0.9, 10),
            ("src/a.rs".into(), "fn b()".into(), 0.8, 50), // same file, different line
        ];
        let deduped = dedup_and_sort(results, 10);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn test_dedup_sorted_by_score_desc() {
        let results = vec![
            ("src/low.rs".into(), "low".into(), 0.3, 1),
            ("src/high.rs".into(), "high".into(), 0.95, 1),
            ("src/mid.rs".into(), "mid".into(), 0.6, 1),
        ];
        let deduped = dedup_and_sort(results, 10);
        assert_eq!(deduped.len(), 3);
        assert!(deduped[0].2 >= deduped[1].2);
        assert!(deduped[1].2 >= deduped[2].2);
    }

    #[test]
    fn test_dedup_respects_limit() {
        let results = vec![
            ("src/a.rs".into(), "a".into(), 0.9, 1),
            ("src/b.rs".into(), "b".into(), 0.8, 1),
            ("src/c.rs".into(), "c".into(), 0.7, 1),
        ];
        let deduped = dedup_and_sort(results, 2);
        assert_eq!(deduped.len(), 2);
        assert!((deduped[0].2 - 0.9).abs() < 0.01);
        assert!((deduped[1].2 - 0.8).abs() < 0.01);
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
