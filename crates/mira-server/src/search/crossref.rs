// crates/mira-server/src/search/crossref.rs
// Cross-reference search (call graph) functionality

use crate::db::Database;
use rusqlite::params;

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
            let name = rest.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !name.is_empty() {
                return Some((name.to_string(), CrossRefType::Caller));
            }
        }
    }

    // Also check for pattern in the middle: "find callers of X"
    for pattern in [" callers of ", " who calls ", " what calls "] {
        if let Some(idx) = q.find(pattern) {
            let rest = &q[idx + pattern.len()..];
            let name = rest.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
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

/// Find functions that call a given symbol
pub fn find_callers(
    db: &Database,
    project_id: Option<i64>,
    target_name: &str,
    limit: usize,
) -> Vec<CrossRefResult> {
    let conn = db.conn();

    // Find all call_graph entries where callee_name matches target
    // Join with code_symbols to get caller details
    let query = if project_id.is_some() {
        "SELECT cs.name, cs.file_path, cg.call_count
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cg.callee_name = ?1 AND cs.project_id = ?2
         ORDER BY cg.call_count DESC
         LIMIT ?3"
    } else {
        "SELECT cs.name, cs.file_path, cg.call_count
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cg.callee_name = ?1
         ORDER BY cg.call_count DESC
         LIMIT ?2"
    };

    let results: Vec<CrossRefResult> = if let Some(pid) = project_id {
        conn.prepare(query)
            .and_then(|mut stmt| {
                stmt.query_map(params![target_name, pid, limit as i64], |row| {
                    Ok(CrossRefResult {
                        symbol_name: row.get(0)?,
                        file_path: row.get(1)?,
                        ref_type: CrossRefType::Caller,
                        call_count: row.get::<_, i32>(2).unwrap_or(1),
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    } else {
        conn.prepare(query)
            .and_then(|mut stmt| {
                stmt.query_map(params![target_name, limit as i64], |row| {
                    Ok(CrossRefResult {
                        symbol_name: row.get(0)?,
                        file_path: row.get(1)?,
                        ref_type: CrossRefType::Caller,
                        call_count: row.get::<_, i32>(2).unwrap_or(1),
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    };

    results
}

/// Check if a function name is a stdlib/utility call that should be filtered
fn is_stdlib_call(name: &str) -> bool {
    // Common Rust std methods and traits
    const STDLIB_NAMES: &[&str] = &[
        // Iterator/collection methods
        "map", "filter", "collect", "iter", "into_iter", "for_each", "fold", "reduce",
        "find", "any", "all", "count", "take", "skip", "chain", "zip", "enumerate",
        "filter_map", "flat_map", "flatten", "peekable", "rev", "cycle",
        // Option/Result methods
        "unwrap", "unwrap_or", "unwrap_or_else", "unwrap_or_default", "expect",
        "ok", "err", "is_some", "is_none", "is_ok", "is_err", "ok_or", "ok_or_else",
        "map_err", "and_then", "or_else", "transpose", "as_ref", "as_mut",
        // Common traits/constructors
        "new", "default", "clone", "to_string", "to_owned", "into", "from",
        "as_str", "as_bytes", "as_slice", "to_vec", "push", "pop", "insert", "remove",
        "get", "get_mut", "contains", "len", "is_empty", "clear", "extend",
        // Result/Option constructors
        "Ok", "Err", "Some", "None",
        // Formatting
        "format", "write", "writeln", "print", "println", "eprint", "eprintln",
        // Logging (without prefix)
        "debug", "info", "warn", "error", "trace",
        // Common string methods
        "split", "join", "trim", "replace", "starts_with", "ends_with", "contains",
        "to_lowercase", "to_uppercase", "parse", "chars", "bytes", "lines",
        // Sync primitives
        "lock", "read", "write", "try_lock", "try_read", "try_write",
        // Async
        "await", "poll", "spawn", "block_on",
        // Math/comparison
        "max", "min", "abs", "cmp", "partial_cmp", "eq", "ne", "lt", "le", "gt", "ge",
        // Database/connection
        "conn", "connection", "execute", "query", "prepare", "query_row", "query_map",
        // Misc
        "drop", "take", "swap", "mem", "ptr", "Box", "Rc", "Arc", "Vec", "String",
        "HashMap", "HashSet", "BTreeMap", "BTreeSet", "VecDeque",
    ];

    // Check exact match
    if STDLIB_NAMES.contains(&name) {
        return true;
    }

    // Check prefixes (logging crates, std types, etc.)
    let prefixes = [
        "tracing::", "log::", "std::", "core::",
        "Vec::", "String::", "HashMap::", "HashSet::", "BTreeMap::", "BTreeSet::",
        "Option::", "Result::", "Box::", "Rc::", "Arc::", "Cell::", "RefCell::",
        "Mutex::", "RwLock::", "Path::", "PathBuf::", "OsStr::", "OsString::",
    ];
    for prefix in prefixes {
        if name.starts_with(prefix) {
            return true;
        }
    }

    false
}

/// Find functions called by a given symbol
pub fn find_callees(
    db: &Database,
    project_id: Option<i64>,
    caller_name: &str,
    limit: usize,
) -> Vec<CrossRefResult> {
    let conn = db.conn();

    // Find the caller symbol(s), then get all their callees
    let query = if project_id.is_some() {
        "SELECT cg.callee_name, cs.file_path, COUNT(*) as cnt
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cs.name = ?1 AND cs.project_id = ?2
         GROUP BY cg.callee_name
         ORDER BY cnt DESC
         LIMIT ?3"
    } else {
        "SELECT cg.callee_name, cs.file_path, COUNT(*) as cnt
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cs.name = ?1
         GROUP BY cg.callee_name
         ORDER BY cnt DESC
         LIMIT ?2"
    };

    let results: Vec<CrossRefResult> = if let Some(pid) = project_id {
        conn.prepare(query)
            .and_then(|mut stmt| {
                stmt.query_map(params![caller_name, pid, limit as i64], |row| {
                    Ok(CrossRefResult {
                        symbol_name: row.get(0)?,
                        file_path: row.get(1)?,
                        ref_type: CrossRefType::Callee,
                        call_count: row.get::<_, i32>(2).unwrap_or(1),
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    } else {
        conn.prepare(query)
            .and_then(|mut stmt| {
                stmt.query_map(params![caller_name, limit as i64], |row| {
                    Ok(CrossRefResult {
                        symbol_name: row.get(0)?,
                        file_path: row.get(1)?,
                        ref_type: CrossRefType::Callee,
                        call_count: row.get::<_, i32>(2).unwrap_or(1),
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    };

    // Filter out stdlib/utility calls
    results.into_iter()
        .filter(|r| !is_stdlib_call(&r.symbol_name))
        .collect()
}

/// Cross-reference search: find callers or callees based on query
/// Returns None if query doesn't match cross-reference patterns
pub fn crossref_search(
    db: &Database,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Option<(String, CrossRefType, Vec<CrossRefResult>)> {
    tracing::debug!(query = %query, "crossref_search: checking query");
    let (target, ref_type) = extract_crossref_target(query)?;
    tracing::info!(target = %target, ref_type = ?ref_type, "crossref_search: pattern matched");

    let results = match ref_type {
        CrossRefType::Caller => find_callers(db, project_id, &target, limit),
        CrossRefType::Callee => find_callees(db, project_id, &target, limit),
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
