// crates/mira-server/src/background/code_health/detection.rs
// Pattern-based detection for code health issues
// Uses pure Rust implementation (no shell commands) for cross-platform support

use crate::db::Database;
use ignore::WalkBuilder;
use regex::Regex;
use rusqlite::params;
use std::fs;
use std::path::Path;

/// Walk Rust files in a project, respecting .gitignore
fn walk_rust_files(project_path: &str) -> Vec<String> {
    let prefix = project_path.to_string();
    WalkBuilder::new(project_path)
        .hidden(true) // Skip hidden files
        .git_ignore(true) // Respect .gitignore
        .git_exclude(true)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "rs")
        })
        .filter(|entry| {
            let path = entry.path();
            // Skip target directory explicitly (in case .gitignore is missing)
            !path.components().any(|c| c.as_os_str() == "target")
        })
        .filter_map(|entry| {
            entry
                .path()
                .strip_prefix(&prefix)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .collect()
}

/// Scan for TODO/FIXME/HACK comments
pub fn scan_todo_comments(
    db: &Database,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let pattern =
        Regex::new(r"(TODO|FIXME|HACK|XXX)(\([^)]+\))?:").map_err(|e| e.to_string())?;

    let mut stored = 0;

    for file in walk_rust_files(project_path) {
        let full_path = Path::new(project_path).join(&file);
        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for (line_num, line) in content.lines().enumerate() {
            let line_num = line_num + 1; // 1-indexed

            if pattern.is_match(line) {
                let comment = line.trim();
                let content = format!("[todo] {}:{} - {}", file, line_num, comment);
                let key = format!("health:todo:{}:{}", file, line_num);

                db.store_memory(
                    Some(project_id),
                    Some(&key),
                    &content,
                    "health",
                    Some("todo"),
                    0.7, // Lower confidence - TODOs are informational
                )
                .map_err(|e| e.to_string())?;

                stored += 1;

                // Limit to prevent flooding
                if stored >= 50 {
                    return Ok(stored);
                }
            }
        }
    }

    Ok(stored)
}

/// Scan for unimplemented!() and todo!() macros
pub fn scan_unimplemented(
    db: &Database,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let pattern =
        Regex::new(r"(unimplemented!|todo!)\s*\(").map_err(|e| e.to_string())?;

    let mut stored = 0;

    for file in walk_rust_files(project_path) {
        let full_path = Path::new(project_path).join(&file);
        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for (line_num, line) in content.lines().enumerate() {
            let line_num = line_num + 1;
            let code = line.trim();

            // Skip comments (doc comments and regular comments)
            if code.starts_with("//") || code.starts_with("/*") || code.starts_with('*') {
                continue;
            }

            if pattern.is_match(line) {
                let content = format!("[unimplemented] {}:{} - {}", file, line_num, code);
                let key = format!("health:unimplemented:{}:{}", file, line_num);

                db.store_memory(
                    Some(project_id),
                    Some(&key),
                    &content,
                    "health",
                    Some("unimplemented"),
                    0.8,
                )
                .map_err(|e| e.to_string())?;

                stored += 1;

                if stored >= 20 {
                    return Ok(stored);
                }
            }
        }
    }

    Ok(stored)
}

/// Find functions that are never called (using indexed call graph)
/// Note: This is heuristic-based since the call graph doesn't capture self.method() calls
pub fn scan_unused_functions(db: &Database, project_id: i64) -> Result<usize, String> {
    // Query unused functions (release connection before storing)
    let unused: Vec<(String, String, i64)> = {
        let conn = db.conn();

        // Find functions that are defined but never appear as callees
        // The call graph doesn't capture self.method() calls, so we use heuristics:
        // - Exclude common method patterns (process_*, handle_*, get_*, etc.)
        // - Exclude trait implementations and common entry points
        // - Exclude test functions
        let mut stmt = conn
            .prepare(
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
            )
            .map_err(|e| e.to_string())?;

        stmt.query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect()
    }; // conn dropped here

    let mut stored = 0;

    for (name, file_path, line) in unused {
        let content = format!(
            "[unused] Function `{}` at {}:{} appears to have no callers",
            name, file_path, line
        );
        let key = format!("health:unused:{}:{}", file_path, name);

        db.store_memory(
            Some(project_id),
            Some(&key),
            &content,
            "health",
            Some("unused"),
            0.5, // Low confidence - call graph doesn't capture self.method() calls
        )
        .map_err(|e| e.to_string())?;

        stored += 1;
    }

    Ok(stored)
}

/// Scan for .unwrap() and .expect() calls in non-test code
/// These are potential panic points that should use proper error handling
pub fn scan_unwrap_usage(
    db: &Database,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let mut stored = 0;

    for file in walk_rust_files(project_path) {
        // Skip test files entirely
        if file.contains("/tests/") || file.ends_with("_test.rs") {
            continue;
        }

        let full_path = Path::new(project_path).join(&file);
        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Track if we're inside a #[cfg(test)] module
        let mut in_test_module = false;
        let mut brace_depth = 0;
        let mut test_module_start_depth = 0;

        for (line_num, line) in content.lines().enumerate() {
            let line_num = line_num + 1; // 1-indexed
            let trimmed = line.trim();

            // Track #[cfg(test)] modules
            if trimmed.contains("#[cfg(test)]") {
                in_test_module = true;
                test_module_start_depth = brace_depth;
            }

            // Track brace depth for module boundaries
            brace_depth += line.matches('{').count();
            brace_depth = brace_depth.saturating_sub(line.matches('}').count());

            // Exit test module when we close its braces
            if in_test_module && brace_depth <= test_module_start_depth && trimmed.contains('}') {
                in_test_module = false;
            }

            // Skip if in test module or test function
            if in_test_module
                || trimmed.starts_with("#[test]")
                || trimmed.starts_with("#[tokio::test]")
            {
                continue;
            }

            // Skip comments
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }

            // Check for unwrap/expect
            let has_unwrap = line.contains(".unwrap()");
            let has_expect = line.contains(".expect(");

            if has_unwrap || has_expect {
                // Determine severity based on context
                let (severity, pattern) = if has_expect {
                    ("medium", "expect")
                } else {
                    ("high", "unwrap")
                };

                // Skip some known-safe patterns
                if is_safe_unwrap(line) {
                    continue;
                }

                let content_str = format!(
                    "[{}] .{}() at {}:{} - {}",
                    severity,
                    pattern,
                    file,
                    line_num,
                    trimmed.chars().take(100).collect::<String>()
                );
                let key = format!("health:unwrap:{}:{}", file, line_num);

                db.store_memory(
                    Some(project_id),
                    Some(&key),
                    &content_str,
                    "health",
                    Some("unwrap"),
                    if severity == "high" { 0.85 } else { 0.7 },
                )
                .map_err(|e| e.to_string())?;

                stored += 1;

                // Limit to prevent flooding
                if stored >= 30 {
                    return Ok(stored);
                }
            }
        }
    }

    Ok(stored)
}

/// Check if an unwrap is in a known-safe pattern
fn is_safe_unwrap(line: &str) -> bool {
    let trimmed = line.trim();

    // Skip string literals that contain ".unwrap()" or ".expect(" (e.g., this scanner)
    if trimmed.contains(r#"".unwrap()"#) || trimmed.contains(r#"".expect("#) {
        return true;
    }
    if trimmed.contains(r#"'.unwrap()"#) || trimmed.contains(r#"'.expect("#) {
        return true;
    }

    // Static/const initializers (Selector::parse, Regex::new, etc.)
    if trimmed.contains("Selector::parse(") {
        return true;
    }
    if trimmed.contains("Regex::new(") {
        return true;
    }

    // Mutex/RwLock (poisoning is usually not recoverable anyway)
    if trimmed.contains(".lock().unwrap()")
        || trimmed.contains(".lock().expect(")
        || trimmed.contains(".read().unwrap()")
        || trimmed.contains(".read().expect(")
        || trimmed.contains(".write().unwrap()")
        || trimmed.contains(".write().expect(")
    {
        return true;
    }

    // Channel operations in controlled contexts
    if trimmed.contains(".send(") && (trimmed.contains(".unwrap()") || trimmed.contains(".expect("))
    {
        return true;
    }

    // Parser set_language (static, cannot fail)
    if trimmed.contains("set_language(") {
        return true;
    }

    false
}

/// Pattern-based scan for error handling issues
pub fn scan_error_handling(
    db: &Database,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let mut stored = 0;

    for file in walk_rust_files(project_path) {
        // Skip test files
        if file.contains("/tests/") || file.ends_with("_test.rs") {
            continue;
        }

        let full_path = Path::new(project_path).join(&file);
        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Track test modules
        let mut in_test_module = false;
        let mut brace_depth = 0;
        let mut test_module_start_depth = 0;

        for (line_num, line) in content.lines().enumerate() {
            let line_num = line_num + 1;
            let trimmed = line.trim();

            // Track test modules
            if trimmed.contains("#[cfg(test)]") {
                in_test_module = true;
                test_module_start_depth = brace_depth;
            }
            brace_depth += line.matches('{').count();
            brace_depth = brace_depth.saturating_sub(line.matches('}').count());
            if in_test_module && brace_depth <= test_module_start_depth && trimmed.contains('}') {
                in_test_module = false;
            }
            if in_test_module {
                continue;
            }

            // Skip comments
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }

            // Check for silent error swallowing patterns
            let issue = check_error_pattern(trimmed);
            if let Some((severity, pattern, description)) = issue {
                // Skip known acceptable patterns
                if is_acceptable_error_swallow(trimmed) {
                    continue;
                }

                let content_str = format!(
                    "[{}] {} at {}:{} - {}",
                    severity,
                    description,
                    file,
                    line_num,
                    trimmed.chars().take(80).collect::<String>()
                );
                let key = format!("health:error:{}:{}:{}", pattern, file, line_num);

                db.store_memory(
                    Some(project_id),
                    Some(&key),
                    &content_str,
                    "health",
                    Some("error_handling"),
                    if severity == "high" { 0.8 } else { 0.6 },
                )
                .map_err(|e| e.to_string())?;

                stored += 1;

                if stored >= 20 {
                    return Ok(stored);
                }
            }
        }
    }

    Ok(stored)
}

/// Check for problematic error handling patterns
fn check_error_pattern(line: &str) -> Option<(&'static str, &'static str, &'static str)> {
    // High severity: silently discarding Results
    if line.contains("let _ =")
        && (line.contains("execute(")
            || line.contains("insert(")
            || line.contains("update(")
            || line.contains("delete("))
    {
        return Some((
            "high",
            "silent_db",
            "DB operation result silently discarded",
        ));
    }

    // Medium severity: .ok() on non-optional contexts
    if line.contains(".ok()") && !line.contains(".ok()?") {
        // Skip lines that are just method chain continuations (start with .)
        let trimmed = line.trim();
        if trimmed.starts_with('.') {
            return None;
        }

        // Skip env vars (both std::env::var and env::var), file reads, and parsing
        if line.contains("env::var")
            || line.contains("read_to_string")
            || line.contains("from_str")
            || line.contains("parse::<")
            || line.contains("parse()")
        {
            return None;
        }

        // Check if it's being used to convert Result to Option for control flow
        if !line.contains(".ok().") && !line.contains(".ok()?") {
            return Some((
                "medium",
                "ok_swallow",
                ".ok() may be swallowing important errors",
            ));
        }
    }

    // Medium severity: ignoring send errors on channels (may indicate receiver dropped)
    if line.contains("let _ =") && line.contains(".send(") && !line.contains("// ") {
        // This is often intentional but worth flagging
        return Some((
            "low",
            "send_ignore",
            "Channel send error ignored (receiver may have dropped)",
        ));
    }

    None
}

/// Check if error swallowing is acceptable in context
fn is_acceptable_error_swallow(line: &str) -> bool {
    // Logging before discard
    if line.contains("error!") || line.contains("warn!") || line.contains("tracing::") {
        return true;
    }

    // Explicit comment explaining why
    if line.contains("// intentional")
        || line.contains("// ignore")
        || line.contains("// ok to fail")
    {
        return true;
    }

    // Filter operations (expected to filter out errors)
    if line.contains("filter_map") || line.contains("filter(|") {
        return true;
    }

    // .ok() with explicit fallback handling (intentional conversion to Option)
    if line.contains(".ok().flatten()")
        || line.contains(".ok().unwrap_or")
        || line.contains(".ok().map(")
        || line.contains(".ok().and_then(")
    {
        return true;
    }

    // Database "get" operations often return Option intentionally
    if line.contains(".get_") && line.contains(".ok()") {
        return true;
    }

    false
}
