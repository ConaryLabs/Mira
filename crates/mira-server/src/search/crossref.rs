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
    /// Line where the symbol starts (from code_symbols.start_line)
    pub line: Option<i64>,
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

    // Callee patterns - "callees of X" and "functions called by X" are unambiguous
    for pattern in ["callees of ", "functions called by "] {
        if let Some(rest) = q.strip_prefix(pattern) {
            let name = rest
                .trim()
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !name.is_empty() {
                return Some((name.to_string(), CrossRefType::Callee));
            }
        }
    }

    // "what does X call" - strip trailing punctuation, then check for " call" suffix
    if let Some(rest) = q.strip_prefix("what does ") {
        let name = rest.split_whitespace().next()?;
        let name = name.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        let q_trimmed = q.trim_end_matches(|c: char| !c.is_alphanumeric());
        if !name.is_empty() && q_trimmed.ends_with(" call") {
            return Some((name.to_string(), CrossRefType::Callee));
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
) -> rusqlite::Result<Vec<CrossRefResult>> {
    Ok(find_callers_sync(conn, target_name, project_id, limit)?
        .into_iter()
        .map(|r| CrossRefResult {
            symbol_name: r.symbol_name,
            file_path: r.file_path,
            ref_type: CrossRefType::Caller,
            call_count: r.call_count,
            line: r.line,
        })
        .collect())
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
) -> rusqlite::Result<Vec<CrossRefResult>> {
    Ok(find_callees_sync(conn, caller_name, project_id, limit)?
        .into_iter()
        .map(|r| CrossRefResult {
            symbol_name: r.symbol_name,
            file_path: r.file_path,
            ref_type: CrossRefType::Callee,
            call_count: r.call_count,
            line: r.line,
        })
        // Filter out stdlib/utility calls
        .filter(|r| !is_stdlib_call(&r.symbol_name))
        .collect())
}

/// Cross-reference search: find callers or callees based on query (connection-based)
///
/// Returns `Ok(None)` if query doesn't match cross-reference patterns.
/// Returns `Ok(Some(...))` on a successful pattern match + DB lookup.
/// Returns `Err(...)` if the pattern matched but the DB query failed.
pub fn crossref_search(
    conn: &Connection,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Option<(String, CrossRefType, Vec<CrossRefResult>)>> {
    tracing::debug!(query = %query, "crossref_search: checking query");
    let Some((target, ref_type)) = extract_crossref_target(query) else {
        return Ok(None);
    };
    tracing::info!(target = %target, ref_type = ?ref_type, "crossref_search: pattern matched");

    let results = match ref_type {
        CrossRefType::Caller => find_callers(conn, project_id, &target, limit)?,
        CrossRefType::Callee => find_callees(conn, project_id, &target, limit)?,
    };

    Ok(Some((target, ref_type, results)))
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
        let location = if let Some(line) = result.line {
            format!("{}:{}", result.file_path, line)
        } else {
            result.file_path.clone()
        };
        response.push_str(&format!(
            "{}. `{}` in {} ({}x)\n",
            i + 1,
            result.symbol_name,
            location,
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
    fn test_extract_callee_what_does_call_punctuation() {
        let result = extract_crossref_target("what does process call?");
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
        assert_eq!(result, Some(("handler".to_string(), CrossRefType::Callee)));
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
    fn test_is_stdlib_matches_known_stdlib() {
        // Exact name matches
        for name in [
            "map",
            "filter",
            "collect",
            "iter",
            "fold",
            "unwrap",
            "unwrap_or",
            "expect",
            "ok",
            "is_some",
            "is_none",
            "new",
            "default",
            "clone",
            "to_string",
            "from",
            "into",
            "Ok",
            "Err",
            "Some",
            "None",
            "debug",
            "info",
            "warn",
            "error",
        ] {
            assert!(is_stdlib_call(name), "{} should be stdlib", name);
        }
        // Prefixed matches
        for name in [
            "tracing::info",
            "std::mem::drop",
            "Vec::new",
            "HashMap::new",
        ] {
            assert!(is_stdlib_call(name), "{} should be stdlib", name);
        }
    }

    #[test]
    fn test_is_stdlib_rejects_user_code() {
        for name in [
            "process_request",
            "handle_message",
            "my_custom_function",
            "DatabaseConnection",
            "tracing_helper",
            "std_parser",
        ] {
            assert!(!is_stdlib_call(name), "{} should NOT be stdlib", name);
        }
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
                line: Some(10),
            },
            CrossRefResult {
                symbol_name: "process".to_string(),
                file_path: "src/lib.rs".to_string(),
                ref_type: CrossRefType::Caller,
                call_count: 1,
                line: None,
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
            line: Some(5),
        }];
        let output = format_crossref_results("main", CrossRefType::Callee, &results);

        assert!(output.contains("Functions called by `main`"));
        assert!(output.contains("helper"));
        assert!(output.contains("src/utils.rs"));
    }

    // ============================================================================
    // Integration tests - DB-backed find_callers / find_callees / crossref_search
    // ============================================================================

    use crate::db::test_support::{seed_call_edge, seed_symbol, setup_test_connection};

    /// Set up a test connection with both main and code schemas.
    /// The code_symbols and call_graph tables live in the code DB schema,
    /// so we must create them in addition to the main migrations.
    fn setup_connection_with_code_schema() -> Connection {
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
            CREATE TABLE IF NOT EXISTS call_graph (
                id INTEGER PRIMARY KEY,
                caller_id INTEGER REFERENCES code_symbols(id),
                callee_name TEXT NOT NULL,
                callee_id INTEGER REFERENCES code_symbols(id),
                call_count INTEGER DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_calls_caller ON call_graph(caller_id);
            CREATE INDEX IF NOT EXISTS idx_calls_callee ON call_graph(callee_id);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_find_callers_single() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let handler_id = seed_symbol(
            &conn,
            project_id,
            "handler",
            "src/api.rs",
            "function",
            1,
            10,
        );
        seed_call_edge(&conn, handler_id, "process_request");

        let results = find_callers(&conn, Some(project_id), "process_request", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol_name, "handler");
        assert_eq!(results[0].file_path, "src/api.rs");
        assert_eq!(results[0].ref_type, CrossRefType::Caller);
    }

    #[test]
    fn test_find_callers_multiple() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let a_id = seed_symbol(&conn, project_id, "a", "src/a.rs", "function", 1, 5);
        let b_id = seed_symbol(&conn, project_id, "b", "src/b.rs", "function", 1, 5);
        let c_id = seed_symbol(&conn, project_id, "c", "src/c.rs", "function", 1, 5);
        seed_call_edge(&conn, a_id, "target");
        seed_call_edge(&conn, b_id, "target");
        seed_call_edge(&conn, c_id, "target");

        let results = find_callers(&conn, Some(project_id), "target", 10).unwrap();
        assert_eq!(results.len(), 3);
        let names: Vec<&str> = results.iter().map(|r| r.symbol_name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
        assert!(names.contains(&"c"));
    }

    #[test]
    fn test_find_callers_none() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let results = find_callers(&conn, Some(project_id), "nonexistent_function", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_find_callees_single() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let main_id = seed_symbol(&conn, project_id, "main", "src/main.rs", "function", 1, 20);
        seed_call_edge(&conn, main_id, "init");

        let results = find_callees(&conn, Some(project_id), "main", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol_name, "init");
        assert_eq!(results[0].ref_type, CrossRefType::Callee);
    }

    #[test]
    fn test_find_callees_multiple() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let main_id = seed_symbol(&conn, project_id, "main", "src/main.rs", "function", 1, 30);
        seed_call_edge(&conn, main_id, "init");
        seed_call_edge(&conn, main_id, "run_server");
        seed_call_edge(&conn, main_id, "shutdown");

        let results = find_callees(&conn, Some(project_id), "main", 10).unwrap();
        assert_eq!(results.len(), 3);
        let names: Vec<&str> = results.iter().map(|r| r.symbol_name.as_str()).collect();
        assert!(names.contains(&"init"));
        assert!(names.contains(&"run_server"));
        assert!(names.contains(&"shutdown"));
    }

    #[test]
    fn test_find_callees_filters_stdlib() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let handler_id = seed_symbol(
            &conn,
            project_id,
            "handler",
            "src/api.rs",
            "function",
            1,
            15,
        );
        seed_call_edge(&conn, handler_id, "unwrap");
        seed_call_edge(&conn, handler_id, "my_function");
        seed_call_edge(&conn, handler_id, "clone");
        seed_call_edge(&conn, handler_id, "to_string");

        let results = find_callees(&conn, Some(project_id), "handler", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol_name, "my_function");
    }

    #[test]
    fn test_crossref_search_who_calls() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let handler_id = seed_symbol(
            &conn,
            project_id,
            "handler",
            "src/api.rs",
            "function",
            1,
            10,
        );
        seed_call_edge(&conn, handler_id, "process_request");

        let result =
            crossref_search(&conn, "who calls process_request", Some(project_id), 10).unwrap();
        assert!(result.is_some());
        let (target, ref_type, results) = result.unwrap();
        assert_eq!(target, "process_request");
        assert_eq!(ref_type, CrossRefType::Caller);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol_name, "handler");
    }

    #[test]
    fn test_crossref_search_what_does_call() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let main_id = seed_symbol(&conn, project_id, "main", "src/main.rs", "function", 1, 20);
        seed_call_edge(&conn, main_id, "init");
        seed_call_edge(&conn, main_id, "run_server");

        let result = crossref_search(&conn, "what does main call", Some(project_id), 10).unwrap();
        assert!(result.is_some());
        let (target, ref_type, results) = result.unwrap();
        assert_eq!(target, "main");
        assert_eq!(ref_type, CrossRefType::Callee);
        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol_name.as_str()).collect();
        assert!(names.contains(&"init"));
        assert!(names.contains(&"run_server"));
    }

    #[test]
    fn test_project_isolation() {
        let conn = setup_connection_with_code_schema();
        let (project_a, _) =
            crate::db::get_or_create_project_sync(&conn, "/project/a", Some("proj_a")).unwrap();
        let (project_b, _) =
            crate::db::get_or_create_project_sync(&conn, "/project/b", Some("proj_b")).unwrap();

        let handler_id = seed_symbol(&conn, project_a, "handler", "src/api.rs", "function", 1, 10);
        seed_call_edge(&conn, handler_id, "process_request");

        // Query with project_a should find the caller
        let results_a = find_callers(&conn, Some(project_a), "process_request", 10).unwrap();
        assert_eq!(results_a.len(), 1);

        // Query with project_b should find nothing
        let results_b = find_callers(&conn, Some(project_b), "process_request", 10).unwrap();
        assert!(results_b.is_empty());
    }

    #[test]
    fn test_is_stdlib_prefix_not_false_positive() {
        // "tracing_helper" contains "tracing" but is NOT "tracing::" prefixed
        assert!(!is_stdlib_call("tracing_helper"));
        // Confirm actual stdlib prefix still works
        assert!(is_stdlib_call("tracing::info"));
        // More edge cases: names that start with stdlib type names but aren't qualified
        assert!(!is_stdlib_call("vec_utils"));
        assert!(!is_stdlib_call("hash_map_builder"));
        assert!(!is_stdlib_call("string_parser"));
    }

    // ============================================================================
    // Edge case: empty call graph
    // ============================================================================

    #[test]
    fn test_find_callers_empty_call_graph() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        // No symbols or edges at all
        let results = find_callers(&conn, Some(project_id), "anything", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_find_callees_empty_call_graph() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let results = find_callees(&conn, Some(project_id), "anything", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_crossref_search_no_match_returns_none() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        // Query that doesn't match any crossref pattern
        let result = crossref_search(&conn, "explain this code", Some(project_id), 10).unwrap();
        assert!(result.is_none(), "non-crossref query should return None");
    }

    // ============================================================================
    // Edge case: extract_crossref_target with edge inputs
    // ============================================================================

    #[test]
    fn test_extract_crossref_target_pattern_with_only_whitespace_name() {
        // "who calls " followed by only whitespace => name becomes empty after trim
        let result = extract_crossref_target("who calls    ");
        assert!(
            result.is_none(),
            "whitespace-only target should return None"
        );
    }

    #[test]
    fn test_extract_crossref_target_with_special_characters() {
        // Name with underscores and numbers should work
        let result = extract_crossref_target("callers of my_func_2");
        assert_eq!(
            result,
            Some(("my_func_2".to_string(), CrossRefType::Caller))
        );
    }

    #[test]
    fn test_extract_crossref_what_does_call_without_call_suffix() {
        // "what does X" without " call" suffix should NOT match callee
        let result = extract_crossref_target("what does process do");
        assert!(
            result.is_none(),
            "what does X without 'call' suffix should not match"
        );
    }

    // ============================================================================
    // Edge case: format_crossref_results with single result
    // ============================================================================

    #[test]
    fn test_format_crossref_results_single_result_numbering() {
        let results = vec![CrossRefResult {
            symbol_name: "only_caller".to_string(),
            file_path: "src/single.rs".to_string(),
            ref_type: CrossRefType::Caller,
            call_count: 1,
            line: None,
        }];
        let output = format_crossref_results("target", CrossRefType::Caller, &results);
        assert!(output.contains("1. `only_caller`"));
        assert!(output.contains("(1x)"));
    }

    // ============================================================================
    // Edge case: find_callees with all stdlib calls (all filtered out)
    // ============================================================================

    #[test]
    fn test_find_callees_all_stdlib_returns_empty() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let fn_id = seed_symbol(&conn, project_id, "my_fn", "src/lib.rs", "function", 1, 10);
        // All callees are stdlib
        seed_call_edge(&conn, fn_id, "clone");
        seed_call_edge(&conn, fn_id, "unwrap");
        seed_call_edge(&conn, fn_id, "to_string");
        seed_call_edge(&conn, fn_id, "map");

        let results = find_callees(&conn, Some(project_id), "my_fn", 10).unwrap();
        assert!(
            results.is_empty(),
            "all-stdlib callees should be filtered out"
        );
    }

    // ============================================================================
    // Edge case: find_callers with limit=0
    // ============================================================================

    #[test]
    fn test_find_callers_limit_zero() {
        let conn = setup_connection_with_code_schema();
        let (project_id, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();

        let a_id = seed_symbol(&conn, project_id, "a", "src/a.rs", "function", 1, 5);
        seed_call_edge(&conn, a_id, "target");

        // limit=0 should return empty
        let results = find_callers(&conn, Some(project_id), "target", 0).unwrap();
        assert!(results.is_empty(), "limit=0 should return empty");
    }
}
