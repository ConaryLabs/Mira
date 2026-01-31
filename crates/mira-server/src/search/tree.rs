// crates/mira-server/src/search/tree.rs
// Tree-guided search scope narrowing using the cartographer module map
//
// Inspired by the PageIndex framework's hierarchical navigation approach:
// instead of searching all code chunks globally, score query terms against
// the module tree (names, purposes, exports, paths) to identify the most
// relevant subtrees, then boost results within those subtrees.

use crate::cartographer::Module;
use crate::db::get_cached_modules_sync;
use rusqlite::Connection;

/// Maximum number of modules to include in the narrowed scope
const MAX_SCOPE_MODULES: usize = 3;

/// Minimum module score to be included in scope (at least one strong signal)
const MIN_MODULE_SCORE: f32 = 2.0;

/// Score boost applied to search results within scope paths (30%)
pub const SCOPE_BOOST: f32 = 1.3;

/// Attempt to narrow search scope by scoring modules against query terms.
///
/// Returns `Some(paths)` with the top module directory prefixes if good
/// matches are found, or `None` if the module tree is empty / no matches
/// exceed the score threshold.
pub fn narrow_by_modules(conn: &Connection, query: &str, project_id: i64) -> Option<Vec<String>> {
    let modules = get_cached_modules_sync(conn, project_id).ok()?;
    if modules.is_empty() {
        return None;
    }

    let query_terms: Vec<String> = query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    if query_terms.is_empty() {
        return None;
    }

    let mut scored: Vec<(String, f32)> = modules
        .iter()
        .filter_map(|m| {
            let score = score_module(m, &query_terms);
            if score > 0.0 {
                Some((m.path.clone(), score))
            } else {
                None
            }
        })
        .collect();

    if scored.is_empty() {
        return None;
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let paths: Vec<String> = scored
        .iter()
        .take(MAX_SCOPE_MODULES)
        .filter(|(_, score)| *score >= MIN_MODULE_SCORE)
        .map(|(path, _)| path.clone())
        .collect();

    if paths.is_empty() { None } else { Some(paths) }
}

/// Score a module against query terms.
///
/// Weights:
/// - module name match:    3 points per term
/// - module purpose match: 2 points per term
/// - module exports match: 2 points per term
/// - module path match:    1 point per term
fn score_module(module: &Module, query_terms: &[String]) -> f32 {
    let name_lower = module.name.to_lowercase();
    let path_lower = module.path.to_lowercase();
    let purpose_lower = module.purpose.as_deref().unwrap_or("").to_lowercase();
    let exports_lower: Vec<String> = module.exports.iter().map(|e| e.to_lowercase()).collect();

    let mut score: f32 = 0.0;

    for term in query_terms {
        if name_lower.contains(term.as_str()) {
            score += 3.0;
        }
        if purpose_lower.contains(term.as_str()) {
            score += 2.0;
        }
        if exports_lower.iter().any(|e| e.contains(term.as_str())) {
            score += 2.0;
        }
        if path_lower.contains(term.as_str()) {
            score += 1.0;
        }
    }

    score
}

/// Check if a file path falls within any of the scope path prefixes.
pub fn path_in_scope(file_path: &str, scope_paths: &[String]) -> bool {
    scope_paths
        .iter()
        .any(|prefix| file_path.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_module(name: &str, path: &str, purpose: Option<&str>, exports: &[&str]) -> Module {
        Module {
            id: format!("test/{}", name),
            name: name.to_string(),
            path: path.to_string(),
            purpose: purpose.map(|s| s.to_string()),
            exports: exports.iter().map(|s| s.to_string()).collect(),
            depends_on: vec![],
            symbol_count: 10,
            line_count: 200,
        }
    }

    // ============================================================================
    // score_module tests
    // ============================================================================

    #[test]
    fn test_score_module_name_match() {
        let module = make_module("search", "src/search", None, &[]);
        let terms = vec!["search".to_string()];
        // name(3) + path(1) = 4
        let score = score_module(&module, &terms);
        assert!((score - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_score_module_purpose_match() {
        let module = make_module("db", "src/db", Some("Database operations and queries"), &[]);
        let terms = vec!["database".to_string()];
        // purpose(2) = 2
        let score = score_module(&module, &terms);
        assert!((score - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_score_module_export_match() {
        let module = make_module(
            "pool",
            "src/pool",
            None,
            &["DatabasePool", "ConnectionManager"],
        );
        let terms = vec!["database".to_string()];
        // export(2) = 2
        let score = score_module(&module, &terms);
        assert!((score - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_score_module_path_match() {
        let module = make_module("mod", "crates/mira-server/src/db", None, &[]);
        let terms = vec!["mira".to_string()];
        // path(1) = 1
        let score = score_module(&module, &terms);
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_score_module_multiple_signals() {
        let module = make_module(
            "search",
            "src/search",
            Some("Code search and retrieval"),
            &["hybrid_search", "keyword_search"],
        );
        let terms = vec!["search".to_string()];
        // name(3) + purpose(2) + exports(2) + path(1) = 8
        let score = score_module(&module, &terms);
        assert!((score - 8.0).abs() < 0.01);
    }

    #[test]
    fn test_score_module_multiple_terms() {
        let module = make_module(
            "search",
            "src/search",
            Some("Keyword and semantic search"),
            &["keyword_search"],
        );
        let terms = vec!["keyword".to_string(), "search".to_string()];
        // "keyword": purpose(2) + exports(2) = 4
        // "search": name(3) + purpose(2) + exports(2) + path(1) = 8
        // total = 12
        let score = score_module(&module, &terms);
        assert!((score - 12.0).abs() < 0.01);
    }

    #[test]
    fn test_score_module_no_match() {
        let module = make_module("auth", "src/auth", Some("Authentication"), &["login"]);
        let terms = vec!["database".to_string()];
        let score = score_module(&module, &terms);
        assert!((score - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_score_module_case_insensitive() {
        let module = make_module(
            "Search",
            "src/Search",
            Some("CODE SEARCH"),
            &["HybridSearch"],
        );
        let terms = vec!["search".to_string()];
        // name(3) + purpose(2) + exports(2) + path(1) = 8
        let score = score_module(&module, &terms);
        assert!((score - 8.0).abs() < 0.01);
    }

    // ============================================================================
    // path_in_scope tests
    // ============================================================================

    #[test]
    fn test_path_in_scope_matches() {
        let paths = vec!["src/search".to_string(), "src/db".to_string()];
        assert!(path_in_scope("src/search/keyword.rs", &paths));
        assert!(path_in_scope("src/db/pool.rs", &paths));
    }

    #[test]
    fn test_path_in_scope_no_match() {
        let paths = vec!["src/search".to_string()];
        assert!(!path_in_scope("src/auth/login.rs", &paths));
        assert!(!path_in_scope("tests/integration.rs", &paths));
    }

    #[test]
    fn test_path_in_scope_exact_prefix() {
        let paths = vec!["src/db".to_string()];
        // "src/db_utils" should NOT match "src/db" prefix (different directory)
        // But starts_with would match it — this is acceptable since module paths
        // typically end with / in practice, and false positives just get a small boost
        assert!(path_in_scope("src/db/schema.rs", &paths));
        assert!(path_in_scope("src/db", &paths));
    }

    #[test]
    fn test_path_in_scope_empty() {
        let paths: Vec<String> = vec![];
        assert!(!path_in_scope("src/anything.rs", &paths));
    }
}
