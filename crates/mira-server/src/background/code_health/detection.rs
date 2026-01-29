// crates/mira-server/src/background/code_health/detection.rs
// Pattern-based detection for code health issues
// Uses pure Rust implementation (no shell commands) for cross-platform support

use crate::db::{StoreMemoryParams, get_unused_functions_sync, store_memory_sync};
use crate::project_files::walker;
use crate::utils::ResultExt;
use regex::Regex;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

/// Maximum TODO/FIXME/HACK findings to store per scan
const MAX_TODO_FINDINGS: usize = 50;
/// Maximum unimplemented!() / todo!() findings to store per scan
const MAX_UNIMPLEMENTED_FINDINGS: usize = 20;
/// Maximum .unwrap() / .expect() findings to store per scan
const MAX_UNWRAP_FINDINGS: usize = 30;
/// Maximum error handling findings to store per scan
const MAX_ERROR_HANDLING_FINDINGS: usize = 20;

/// Confidence level for TODO comment findings
const CONFIDENCE_TODO: f64 = 0.7;
/// Confidence level for unimplemented macro findings
const CONFIDENCE_UNIMPLEMENTED: f64 = 0.8;
/// Confidence level for unused function findings (heuristic-based)
const CONFIDENCE_UNUSED: f64 = 0.5;
/// Confidence level for high-severity unwrap findings
const CONFIDENCE_UNWRAP_HIGH: f64 = 0.85;
/// Confidence level for medium-severity unwrap findings
const CONFIDENCE_UNWRAP_MEDIUM: f64 = 0.7;
/// Confidence level for high-severity error handling findings
const CONFIDENCE_ERROR_HIGH: f64 = 0.8;
/// Confidence level for lower-severity error handling findings
const CONFIDENCE_ERROR_LOW: f64 = 0.6;

/// Check if a line contains a #[cfg(...)] attribute that includes `test`
fn is_cfg_test(line: &str) -> bool {
    let line = line.trim();
    let mut search_start = 0;

    while let Some(cfg_start) = line[search_start..].find("#[cfg(") {
        let cfg_start = search_start + cfg_start;
        let mut pos = cfg_start + "#[cfg(".len();
        let mut paren_count = 1;

        // Parse until we find the matching closing parenthesis
        while let Some(ch) = line[pos..].chars().next() {
            match ch {
                '(' => paren_count += 1,
                ')' => {
                    paren_count -= 1;
                    if paren_count == 0 {
                        // Check if next character is ']'
                        if line[pos + 1..].starts_with(']') {
                            let content = &line[cfg_start + "#[cfg(".len()..pos];
                            // Check if content contains "test" as a separate word
                            if content
                                .split(|c: char| !c.is_alphanumeric() && c != '_')
                                .any(|part| part == "test")
                            {
                                return true;
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
            pos += ch.len_utf8();
        }

        // Continue searching after this position
        search_start = cfg_start + 1;
    }

    false
}

/// Walk Rust files in a project, respecting .gitignore
fn walk_rust_files(project_path: &str) -> Result<Vec<String>, String> {
    walker::walk_rust_files(project_path).str_err()
}

/// Scan for TODO/FIXME/HACK comments
pub fn scan_todo_comments(
    conn: &Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let pattern = Regex::new(r"(TODO|FIXME|HACK|XXX)(\([^)]+\))?:").str_err()?;

    let mut stored = 0;

    for file in walk_rust_files(project_path)? {
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

                store_memory_sync(
                    conn,
                    StoreMemoryParams {
                        project_id: Some(project_id),
                        key: Some(&key),
                        content: &content,
                        fact_type: "health",
                        category: Some("todo"),
                        confidence: CONFIDENCE_TODO,
                        session_id: None,
                        user_id: None,
                        scope: "project",
                        branch: None,
                    },
                )
                .str_err()?;

                stored += 1;

                // Limit to prevent flooding
                if stored >= MAX_TODO_FINDINGS {
                    return Ok(stored);
                }
            }
        }
    }

    Ok(stored)
}

/// Scan for unimplemented!() and todo!() macros
pub fn scan_unimplemented(
    conn: &Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let pattern = Regex::new(r"(unimplemented!|todo!)\s*\(").str_err()?;

    let mut stored = 0;

    for file in walk_rust_files(project_path)? {
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

                store_memory_sync(
                    conn,
                    StoreMemoryParams {
                        project_id: Some(project_id),
                        key: Some(&key),
                        content: &content,
                        fact_type: "health",
                        category: Some("unimplemented"),
                        confidence: CONFIDENCE_UNIMPLEMENTED,
                        session_id: None,
                        user_id: None,
                        scope: "project",
                        branch: None,
                    },
                )
                .str_err()?;

                stored += 1;

                if stored >= MAX_UNIMPLEMENTED_FINDINGS {
                    return Ok(stored);
                }
            }
        }
    }

    Ok(stored)
}

/// Find functions that are never called (using indexed call graph)
/// Note: This is heuristic-based since the call graph doesn't capture self.method() calls
pub fn scan_unused_functions(conn: &Connection, project_id: i64) -> Result<usize, String> {
    let unused = get_unused_functions_sync(conn, project_id).str_err()?;

    let mut stored = 0;

    for (name, file_path, line) in unused {
        let content = format!(
            "[unused] Function `{}` at {}:{} appears to have no callers",
            name, file_path, line
        );
        let key = format!("health:unused:{}:{}", file_path, name);

        store_memory_sync(
            conn,
            StoreMemoryParams {
                project_id: Some(project_id),
                key: Some(&key),
                content: &content,
                fact_type: "health",
                category: Some("unused"),
                confidence: CONFIDENCE_UNUSED,
                session_id: None,
                user_id: None,
                scope: "project",
                branch: None,
            },
        )
        .str_err()?;

        stored += 1;
    }

    Ok(stored)
}

/// Scan for .unwrap() and .expect() calls in non-test code
/// These are potential panic points that should use proper error handling
pub fn scan_unwrap_usage(
    conn: &Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let mut stored = 0;

    for file in walk_rust_files(project_path)? {
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
            if is_cfg_test(trimmed) {
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

                store_memory_sync(
                    conn,
                    StoreMemoryParams {
                        project_id: Some(project_id),
                        key: Some(&key),
                        content: &content_str,
                        fact_type: "health",
                        category: Some("unwrap"),
                        confidence: if severity == "high" {
                            CONFIDENCE_UNWRAP_HIGH
                        } else {
                            CONFIDENCE_UNWRAP_MEDIUM
                        },
                        session_id: None,
                        user_id: None,
                        scope: "project",
                        branch: None,
                    },
                )
                .str_err()?;

                stored += 1;

                // Limit to prevent flooding
                if stored >= MAX_UNWRAP_FINDINGS {
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
    conn: &Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let mut stored = 0;

    for file in walk_rust_files(project_path)? {
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
            if is_cfg_test(trimmed) {
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

                store_memory_sync(
                    conn,
                    StoreMemoryParams {
                        project_id: Some(project_id),
                        key: Some(&key),
                        content: &content_str,
                        fact_type: "health",
                        category: Some("error_handling"),
                        confidence: if severity == "high" {
                            CONFIDENCE_ERROR_HIGH
                        } else {
                            CONFIDENCE_ERROR_LOW
                        },
                        session_id: None,
                        user_id: None,
                        scope: "project",
                        branch: None,
                    },
                )
                .str_err()?;

                stored += 1;

                if stored >= MAX_ERROR_HANDLING_FINDINGS {
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
