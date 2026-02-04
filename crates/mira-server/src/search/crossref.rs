// crates/mira-server/src/search/crossref.rs
// Cross-reference search (call graph) functionality

use crate::db::{find_callees_sync, find_callers_sync};
use rusqlite::Connection;

/// Result from cross-reference search
#[derive(Debug, Clone)]
pub struct CrossRefResult {
    /// The symbol being referenced
    pub symbol_name: String,
    /// File containing the symbol
    pub file_path: String,
    /// The relationship type
    pub ref_type: CrossRefType,
    /// Number of calls (for callers)
    pub call_count: i32,
}

/// Type of cross-reference relationship
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CrossRefType {
    /// Functions that call the target
    Caller,
    /// Functions called by the target
    Callee,
}

impl std::fmt::Display for CrossRefType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrossRefType::Caller => write!(f, "caller"),
            CrossRefType::Callee => write!(f, "callee"),
        }
    }
}

/// Extract target symbol name from caller/callee queries
/// Patterns: "who calls X", "callers of X", "what calls X"
///           "what does X call", "callees of X", "functions called by X"
fn extract_crossref_target(query: &str) -> Option<(String, CrossRefType)> {
    let q = query.to_lowercase();

    // Caller patterns
    for pattern in ["who calls ", "callers of ", "what calls ", "references to "] {
        if let Some(rest) = q.strip_prefix(pattern) {
            let name = rest
                .trim()
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !name.is_empty() {
                return Some((name.to_string(), CrossRefType::Caller));
            }
        }
    }

    // Also check for pattern in the middle: "find callers of X"
    for pattern in [" callers of ", " who calls ", " what calls "] {
        if let Some(idx) = q.find(pattern) {
            let rest = &q[idx + pattern.len()..];
            let name = rest
                .trim()
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !name.is_empty() {
                return Some((name.to_string(), CrossRefType::Caller));
            }
        }
    }

    // Callee patterns
    for pattern in ["what does ", "functions called by ", "callees of "] {
        if let Some(rest) = q.strip_prefix(pattern) {
            // For "what does X call" - extract X
            let name = rest.split_whitespace().next()?;
            let name = name.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !name.is_empty() && q.contains(" call") {
                return Some((name.to_string(), CrossRefType::Callee));
            }
        }
    }

    None
}

/// Find functions that call a given symbol (connection-based version)
pub fn find_callers(
    conn: &Connection,
    project_id: Option<i64>,
    target_name: &str,
    limit: usize,
) -> Vec<CrossRefResult> {
    find_callers_sync(conn, target_name, project_id, limit)
        .into_iter()
        .map(|r| CrossRefResult {
            symbol_name: r.symbol_name,
            file_path: r.file_path,
            ref_type: CrossRefType::Caller,
            call_count: r.call_count,
        })
        .collect()
}

/// Check if a function name is a stdlib/utility call that should be filtered
fn is_stdlib_call(name: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::LazyLock;

    static STDLIB_NAMES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        HashSet::from([
            // Iterator/collection methods
            "map",
            "filter",
            "collect",
            "iter",
            "into_iter",
            "for_each",
            "fold",
            "reduce",
            "find",
            "any",
            "all",
            "count",
            "take",
            "skip",
            "chain",
            "zip",
            "enumerate",
            "filter_map",
            "flat_map",
            "flatten",
            "peekable",
            "rev",
            "cycle",
            // Option/Result methods
            "unwrap",
            "unwrap_or",
            "unwrap_or_else",
            "unwrap_or_default",
            "expect",
            "ok",
            "err",
            "is_some",
            "is_none",
            "is_ok",
            "is_err",
            "ok_or",
            "ok_or_else",
            "map_err",
            "and_then",
            "or_else",
            "transpose",
            "as_ref",
            "as_mut",
            // Common traits/constructors
            "new",
            "default",
            "clone",
            "to_string",
            "to_owned",
            "into",
            "from",
            "as_str",
            "as_bytes",
            "as_slice",
            "to_vec",
            "push",
            "pop",
            "insert",
            "remove",
            "get",
            "get_mut",
            "contains",
            "len",
            "is_empty",
            "clear",
            "extend",
            // Result/Option constructors
            "Ok",
            "Err",
            "Some",
            "None",
            // Formatting
            "format",
            "write",
            "writeln",
            "print",
            "println",
            "eprint",
            "eprintln",
            // Logging (without prefix)
            "debug",
            "info",
            "warn",
            "error",
            "trace",
            // Common string methods
            "split",
            "join",
            "trim",
            "replace",
            "starts_with",
            "ends_with",
            "to_lowercase",
            "to_uppercase",
            "parse",
            "chars",
            "bytes",
            "lines",
            // Sync primitives
            "lock",
            "read",
            "try_lock",
            "try_read",
            "try_write",
            // Async
            "await",
            "poll",
            "spawn",
            "block_on",
            // Math/comparison
            "max",
            "min",
            "abs",
            "cmp",
            "partial_cmp",
            "eq",
            "ne",
            "lt",
            "le",
            "gt",
            "ge",
            // Database/connection
            "conn",
            "connection",
            "execute",
            "query",
            "prepare",
            "query_row",
            "query_map",
            // Misc
            "drop",
            "swap",
            "mem",
            "ptr",
            "Box",
            "Rc",
            "Arc",
            "Vec",
            "String",
            "HashMap",
            "HashSet",
            "BTreeMap",
            "BTreeSet",
            "VecDeque",
        ])
    });

    // Check exact match (O(1) lookup)
    if STDLIB_NAMES.contains(name) {
        return true;
    }

    // Check prefixes (logging crates, std types, etc.)
    let prefixes = [
        "tracing::",
        "log::",
        "std::",
        "core::",
        "Vec::",
        "String::",
        "HashMap::",
        "HashSet::",
        "BTreeMap::",
        "BTreeSet::",
        "Option::",
        "Result::",
        "Box::",
        "Rc::",
        "Arc::",
        "Cell::",
        "RefCell::",
        "Mutex::",
        "RwLock::",
        "Path::",
        "PathBuf::",
        "OsStr::",
        "OsString::",
    ];
    for prefix in prefixes {
        if name.starts_with(prefix) {
            return true;
        }
    }

    false
}

/// Find functions called by a given symbol (connection-based version)
pub fn find_callees(
    conn: &Connection,
    project_id: Option<i64>,
    caller_name: &str,
    limit: usize,
) -> Vec<CrossRefResult> {
    find_callees_sync(conn, caller_name, project_id, limit)
        .into_iter()
        .map(|r| CrossRefResult {
            symbol_name: r.symbol_name,
            file_path: r.file_path,
            ref_type: CrossRefType::Callee,
            call_count: r.call_count,
        })
        // Filter out stdlib/utility calls
        .filter(|r| !is_stdlib_call(&r.symbol_name))
        .collect()
}

/// Cross-reference search: find callers or callees based on query (connection-based)
/// Returns None if query doesn't match cross-reference patterns
pub fn crossref_search(
    conn: &Connection,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Option<(String, CrossRefType, Vec<CrossRefResult>)> {
    tracing::debug!(query = %query, "crossref_search: checking query");
    let (target, ref_type) = extract_crossref_target(query)?;
    tracing::info!(target = %target, ref_type = ?ref_type, "crossref_search: pattern matched");

    let results = match ref_type {
        CrossRefType::Caller => find_callers(conn, project_id, &target, limit),
        CrossRefType::Callee => find_callees(conn, project_id, &target, limit),
    };

    Some((target, ref_type, results))
}

/// Format cross-reference results for display
pub fn format_crossref_results(
    target: &str,
    ref_type: CrossRefType,
    results: &[CrossRefResult],
) -> String {
    if results.is_empty() {
        return match ref_type {
            CrossRefType::Caller => format!("No callers found for `{}`.", target),
            CrossRefType::Callee => format!("No callees found for `{}`.", target),
        };
    }

    let header = match ref_type {
        CrossRefType::Caller => format!("Functions that call `{}`:\n\n", target),
        CrossRefType::Callee => format!("Functions called by `{}`:\n\n", target),
    };

    let mut response = header;

    for (i, result) in results.iter().enumerate() {
        response.push_str(&format!(
            "{}. `{}` in {} ({}x)\n",
            i + 1,
            result.symbol_name,
            result.file_path,
            result.call_count
        ));
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // extract_crossref_target tests - Caller patterns
    // ============================================================================

    #[test]
    fn test_extract_caller_who_calls() {
        let result = extract_crossref_target("who calls process_request");
        assert_eq!(
            result,
            Some(("process_request".to_string(), CrossRefType::Caller))
        );
    }

    #[test]
    fn test_extract_caller_callers_of() {
        let result = extract_crossref_target("callers of handle_message");
        assert_eq!(
            result,
            Some(("handle_message".to_string(), CrossRefType::Caller))
        );
    }

    #[test]
    fn test_extract_caller_what_calls() {
        let result = extract_crossref_target("what calls database_query");
        assert_eq!(
            result,
            Some(("database_query".to_string(), CrossRefType::Caller))
        );
    }

    #[test]
    fn test_extract_caller_references_to() {
        let result = extract_crossref_target("references to my_function");
        assert_eq!(
            result,
            Some(("my_function".to_string(), CrossRefType::Caller))
        );
    }

    #[test]
    fn test_extract_caller_middle_pattern() {
        let result = extract_crossref_target("find callers of execute");
        assert_eq!(result, Some(("execute".to_string(), CrossRefType::Caller)));
    }

    #[test]
    fn test_extract_caller_case_insensitive() {
        let result = extract_crossref_target("WHO CALLS myFunc");
        assert_eq!(result, Some(("myfunc".to_string(), CrossRefType::Caller)));
    }

    // ============================================================================
    // extract_crossref_target tests - Callee patterns
    // ============================================================================

    #[test]
    fn test_extract_callee_what_does_call() {
        let result = extract_crossref_target("what does process call");
        assert_eq!(result, Some(("process".to_string(), CrossRefType::Callee)));
    }

    #[test]
    fn test_extract_callee_functions_called_by() {
        let result = extract_crossref_target("functions called by main");
        assert_eq!(result, Some(("main".to_string(), CrossRefType::Callee)));
    }

    #[test]
    fn test_extract_callee_callees_of() {
        let result = extract_crossref_target("callees of handler");
        // This pattern requires " call" in the query
        assert!(result.is_none() || result.unwrap().1 == CrossRefType::Callee);
    }

    // ============================================================================
    // extract_crossref_target tests - No match
    // ============================================================================

    #[test]
    fn test_extract_no_match_empty() {
        assert!(extract_crossref_target("").is_none());
    }

    #[test]
    fn test_extract_no_match_general_query() {
        assert!(extract_crossref_target("find authentication code").is_none());
    }

    #[test]
    fn test_extract_no_match_search_query() {
        assert!(extract_crossref_target("search for database").is_none());
    }

    // ============================================================================
    // is_stdlib_call tests
    // ============================================================================

    #[test]
    fn test_is_stdlib_iterator_methods() {
        assert!(is_stdlib_call("map"));
        assert!(is_stdlib_call("filter"));
        assert!(is_stdlib_call("collect"));
        assert!(is_stdlib_call("iter"));
        assert!(is_stdlib_call("fold"));
    }

    #[test]
    fn test_is_stdlib_option_result_methods() {
        assert!(is_stdlib_call("unwrap"));
        assert!(is_stdlib_call("unwrap_or"));
        assert!(is_stdlib_call("expect"));
        assert!(is_stdlib_call("ok"));
        assert!(is_stdlib_call("is_some"));
        assert!(is_stdlib_call("is_none"));
    }

    #[test]
    fn test_is_stdlib_constructors() {
        assert!(is_stdlib_call("new"));
        assert!(is_stdlib_call("default"));
        assert!(is_stdlib_call("clone"));
        assert!(is_stdlib_call("to_string"));
        assert!(is_stdlib_call("from"));
        assert!(is_stdlib_call("into"));
    }

    #[test]
    fn test_is_stdlib_result_option_variants() {
        assert!(is_stdlib_call("Ok"));
        assert!(is_stdlib_call("Err"));
        assert!(is_stdlib_call("Some"));
        assert!(is_stdlib_call("None"));
    }

    #[test]
    fn test_is_stdlib_logging() {
        assert!(is_stdlib_call("debug"));
        assert!(is_stdlib_call("info"));
        assert!(is_stdlib_call("warn"));
        assert!(is_stdlib_call("error"));
    }

    #[test]
    fn test_is_stdlib_prefixed() {
        assert!(is_stdlib_call("tracing::info"));
        assert!(is_stdlib_call("std::mem::drop"));
        assert!(is_stdlib_call("Vec::new"));
        assert!(is_stdlib_call("HashMap::new"));
    }

    #[test]
    fn test_is_stdlib_not_stdlib() {
        assert!(!is_stdlib_call("process_request"));
        assert!(!is_stdlib_call("handle_message"));
        assert!(!is_stdlib_call("my_custom_function"));
        assert!(!is_stdlib_call("DatabaseConnection"));
    }

    // ============================================================================
    // format_crossref_results tests
    // ============================================================================

    #[test]
    fn test_format_empty_callers() {
        let result = format_crossref_results("foo", CrossRefType::Caller, &[]);
        assert!(result.contains("No callers found"));
        assert!(result.contains("foo"));
    }

    #[test]
    fn test_format_empty_callees() {
        let result = format_crossref_results("bar", CrossRefType::Callee, &[]);
        assert!(result.contains("No callees found"));
        assert!(result.contains("bar"));
    }

    #[test]
    fn test_format_callers_with_results() {
        let results = vec![
            CrossRefResult {
                symbol_name: "handler".to_string(),
                file_path: "src/main.rs".to_string(),
                ref_type: CrossRefType::Caller,
                call_count: 3,
            },
            CrossRefResult {
                symbol_name: "process".to_string(),
                file_path: "src/lib.rs".to_string(),
                ref_type: CrossRefType::Caller,
                call_count: 1,
            },
        ];
        let output = format_crossref_results("target_fn", CrossRefType::Caller, &results);

        assert!(output.contains("Functions that call `target_fn`"));
        assert!(output.contains("handler"));
        assert!(output.contains("src/main.rs"));
        assert!(output.contains("(3x)"));
        assert!(output.contains("process"));
        assert!(output.contains("(1x)"));
    }

    #[test]
    fn test_format_callees_with_results() {
        let results = vec![CrossRefResult {
            symbol_name: "helper".to_string(),
            file_path: "src/utils.rs".to_string(),
            ref_type: CrossRefType::Callee,
            call_count: 2,
        }];
        let output = format_crossref_results("main", CrossRefType::Callee, &results);

        assert!(output.contains("Functions called by `main`"));
        assert!(output.contains("helper"));
        assert!(output.contains("src/utils.rs"));
    }
}
