// crates/mira-server/src/background/code_health/detection.rs
// Pattern-based detection for code health issues
// Uses pure Rust implementation (no shell commands) for cross-platform support
//
// All detectors run in a single pass over each file, reducing IO and traversal
// overhead by ~3-4x compared to separate per-detector walks.

use crate::db::{StoreMemoryParams, store_memory_sync};
use crate::project_files;
use crate::utils::ResultExt;
use regex::Regex;
use rusqlite::Connection;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

// ---- Limits ----

/// Maximum TODO/FIXME/HACK findings to store per scan
const MAX_TODO_FINDINGS: usize = 50;
/// Maximum unimplemented!() / todo!() findings to store per scan
const MAX_UNIMPLEMENTED_FINDINGS: usize = 20;
/// Maximum .unwrap() / .expect() findings to store per scan
const MAX_UNWRAP_FINDINGS: usize = 30;
/// Maximum error handling findings to store per scan
const MAX_ERROR_HANDLING_FINDINGS: usize = 20;

// ---- Confidence levels ----

const CONFIDENCE_TODO: f64 = 0.7;
const CONFIDENCE_UNIMPLEMENTED: f64 = 0.8;
const CONFIDENCE_UNWRAP_HIGH: f64 = 0.85;
const CONFIDENCE_UNWRAP_MEDIUM: f64 = 0.7;
const CONFIDENCE_ERROR_HIGH: f64 = 0.8;
const CONFIDENCE_ERROR_LOW: f64 = 0.6;

// ---- Precompiled regexes ----

#[allow(clippy::unwrap_used)] // Infallible: hardcoded regex pattern
static RE_TODO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(TODO|FIXME|HACK|XXX)(\([^)]+\))?:").unwrap());

#[allow(clippy::unwrap_used)] // Infallible: hardcoded regex pattern
static RE_UNIMPLEMENTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(unimplemented!|todo!)\s*\(").unwrap());

/// Results from a single-pass scan of all detection patterns
pub struct DetectionResults {
    pub todos: usize,
    pub unimplemented: usize,
    pub unwraps: usize,
    pub error_handling: usize,
}

/// A collected detection finding, ready for batch storage
pub struct DetectionFinding {
    pub key: String,
    pub content: String,
    pub category: &'static str,
    pub confidence: f64,
}

/// Collected output from scan: counts + findings to store
pub struct DetectionOutput {
    pub results: DetectionResults,
    pub findings: Vec<DetectionFinding>,
}

impl DetectionResults {
    fn all_maxed(&self) -> bool {
        self.todos >= MAX_TODO_FINDINGS
            && self.unimplemented >= MAX_UNIMPLEMENTED_FINDINGS
            && self.unwraps >= MAX_UNWRAP_FINDINGS
            && self.error_handling >= MAX_ERROR_HANDLING_FINDINGS
    }
}

/// Check if `test` appears in a cfg expression as a bare predicate (not inside
/// `not(...)` and not inside a string literal like `feature = "test"`).
fn has_positive_test(expr: &str) -> bool {
    let stripped = strip_not_blocks(expr);
    let unquoted = strip_quoted_strings(&stripped);
    unquoted
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|part| part == "test")
}

/// Remove all `not(...)` sub-expressions from a cfg expression string.
/// Handles optional whitespace between `not` and `(`.
fn strip_not_blocks(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Look for "not" followed by optional whitespace then "("
        if i + 3 <= bytes.len() && &bytes[i..i + 3] == b"not" {
            let mut j = i + 3;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'(' {
                // Skip the entire not(...) block
                j += 1;
                let mut depth = 1;
                while j < bytes.len() && depth > 0 {
                    match bytes[j] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Remove quoted string contents so token matching doesn't match inside them.
/// e.g. `feature = "test"` -> `feature = ""`
fn strip_quoted_strings(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_quote = false;
    for c in s.chars() {
        if c == '"' {
            in_quote = !in_quote;
            result.push(c);
        } else if !in_quote {
            result.push(c);
        }
    }
    result
}

/// Check if a line contains a #[cfg(...)] attribute that gates test-only code.
/// Returns false when `test` only appears inside `not(...)`, since that marks
/// production-only code (e.g. `#[cfg(not(test))]`, `#[cfg(all(unix, not(test)))]`).
fn is_cfg_test(line: &str) -> bool {
    let line = line.trim();
    let mut search_start = 0;

    while let Some(cfg_start) = line[search_start..].find("#[cfg(") {
        let cfg_start = search_start + cfg_start;
        let mut pos = cfg_start + "#[cfg(".len();
        let mut paren_count = 1;

        while let Some(ch) = line[pos..].chars().next() {
            match ch {
                '(' => paren_count += 1,
                ')' => {
                    paren_count -= 1;
                    if paren_count == 0 {
                        if line[pos + 1..].starts_with(']') {
                            let content = &line[cfg_start + "#[cfg(".len()..pos];
                            if has_positive_test(content) {
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

        search_start = cfg_start + 1;
    }

    false
}

/// Walk Rust files in a project, respecting .gitignore
fn walk_rust_files(project_path: &str) -> Result<Vec<String>, String> {
    project_files::walk_rust_files(project_path).str_err()
}

/// Single-pass scan for TODO/FIXME, unimplemented!(), .unwrap(), and error handling patterns.
/// Returns collected findings without writing to DB.
///
/// Walks all Rust files once, reads each file once, and applies all detectors to each line.
pub fn collect_detections(project_path: &str) -> Result<DetectionOutput, String> {
    let mut r = DetectionResults {
        todos: 0,
        unimplemented: 0,
        unwraps: 0,
        error_handling: 0,
    };
    let mut findings = Vec::new();

    for file in walk_rust_files(project_path)? {
        if r.all_maxed() {
            break;
        }

        let skip_test_file = Path::new(&file)
            .components()
            .any(|c| c.as_os_str() == "tests")
            || file.ends_with("_test.rs");

        let full_path = Path::new(project_path).join(&file);
        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Test module tracking (shared by unwrap + error handling detectors)
        let mut in_test_module = false;
        let mut brace_depth: usize = 0;
        let mut test_module_start_depth: usize = 0;

        for (line_idx, line) in content.lines().enumerate() {
            let line_num = line_idx + 1;
            let trimmed = line.trim();

            // ---- Test module tracking ----
            if is_cfg_test(trimmed) {
                in_test_module = true;
                test_module_start_depth = brace_depth;
            }
            brace_depth += line.matches('{').count();
            brace_depth = brace_depth.saturating_sub(line.matches('}').count());
            if in_test_module && brace_depth <= test_module_start_depth && trimmed.contains('}') {
                in_test_module = false;
            }

            let is_comment =
                trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*');

            // ---- 1. TODO/FIXME/HACK (all files, all lines) ----
            if r.todos < MAX_TODO_FINDINGS && RE_TODO.is_match(line) {
                let content_str = format!("[todo] {}:{} - {}", file, line_num, trimmed);
                let key = format!("health:todo:{}:{}", file, line_num);
                findings.push(DetectionFinding {
                    key,
                    content: content_str,
                    category: "todo",
                    confidence: CONFIDENCE_TODO,
                });
                r.todos += 1;
            }

            // ---- 2. unimplemented!/todo! macros (all files, skip comments) ----
            if r.unimplemented < MAX_UNIMPLEMENTED_FINDINGS
                && !is_comment
                && RE_UNIMPLEMENTED.is_match(line)
            {
                let content_str = format!("[unimplemented] {}:{} - {}", file, line_num, trimmed);
                let key = format!("health:unimplemented:{}:{}", file, line_num);
                findings.push(DetectionFinding {
                    key,
                    content: content_str,
                    category: "unimplemented",
                    confidence: CONFIDENCE_UNIMPLEMENTED,
                });
                r.unimplemented += 1;
            }

            // Shared gate: skip test files & test contexts for unwrap + error handling
            let in_test_fn =
                trimmed.starts_with("#[test]") || trimmed.starts_with("#[tokio::test]");

            // ---- 3. .unwrap() / .expect() (non-test code, skip comments) ----
            if r.unwraps < MAX_UNWRAP_FINDINGS
                && !skip_test_file
                && !in_test_module
                && !in_test_fn
                && !is_comment
            {
                let has_unwrap = line.contains(".unwrap()");
                let has_expect = line.contains(".expect(");

                if (has_unwrap || has_expect) && !is_safe_unwrap(line) {
                    let (severity, pattern) = if has_expect {
                        ("medium", "expect")
                    } else {
                        ("high", "unwrap")
                    };

                    let content_str = format!(
                        "[{}] .{}() at {}:{} - {}",
                        severity,
                        pattern,
                        file,
                        line_num,
                        trimmed.chars().take(100).collect::<String>()
                    );
                    let key = format!("health:unwrap:{}:{}", file, line_num);

                    findings.push(DetectionFinding {
                        key,
                        content: content_str,
                        category: "unwrap",
                        confidence: if severity == "high" {
                            CONFIDENCE_UNWRAP_HIGH
                        } else {
                            CONFIDENCE_UNWRAP_MEDIUM
                        },
                    });
                    r.unwraps += 1;
                }
            }

            // ---- 4. Error handling patterns (non-test code, skip comments) ----
            // Note: uses in_test_module but NOT in_test_fn (original behavior preserved)
            if r.error_handling < MAX_ERROR_HANDLING_FINDINGS
                && !skip_test_file
                && !in_test_module
                && !is_comment
                && let Some((severity, pattern, description)) = check_error_pattern(trimmed)
                && !is_acceptable_error_swallow(trimmed)
            {
                let content_str = format!(
                    "[{}] {} at {}:{} - {}",
                    severity,
                    description,
                    file,
                    line_num,
                    trimmed.chars().take(80).collect::<String>()
                );
                let key = format!("health:error:{}:{}:{}", pattern, file, line_num);

                findings.push(DetectionFinding {
                    key,
                    content: content_str,
                    category: "error_handling",
                    confidence: if severity == "high" {
                        CONFIDENCE_ERROR_HIGH
                    } else {
                        CONFIDENCE_ERROR_LOW
                    },
                });
                r.error_handling += 1;
            }
        }
    }

    Ok(DetectionOutput {
        results: r,
        findings,
    })
}

/// Store collected detection findings in the database (batch write).
pub fn store_detection_findings(
    conn: &Connection,
    project_id: i64,
    findings: &[DetectionFinding],
) -> Result<usize, String> {
    for finding in findings {
        store_memory_sync(
            conn,
            StoreMemoryParams {
                project_id: Some(project_id),
                key: Some(&finding.key),
                content: &finding.content,
                fact_type: "health",
                category: Some(finding.category),
                confidence: finding.confidence,
                session_id: None,
                user_id: None,
                scope: "project",
                branch: None,
                team_id: None,
            },
        )
        .str_err()?;
    }
    Ok(findings.len())
}

/// Check if an unwrap is in a known-safe pattern
fn is_safe_unwrap(line: &str) -> bool {
    let trimmed = line.trim();

    // Skip string literals that contain ".unwrap()" or ".expect("
    if trimmed.contains(r#"".unwrap()"#) || trimmed.contains(r#"".expect("#) {
        return true;
    }
    if trimmed.contains(r#"'.unwrap()"#) || trimmed.contains(r#"'.expect("#) {
        return true;
    }

    // Static/const initializers
    if trimmed.contains("Selector::parse(") || trimmed.contains("Regex::new(") {
        return true;
    }

    // Mutex/RwLock (poisoning is usually not recoverable)
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
        let trimmed = line.trim();
        if trimmed.starts_with('.') {
            return None;
        }

        if line.contains("env::var")
            || line.contains("read_to_string")
            || line.contains("from_str")
            || line.contains("parse::<")
            || line.contains("parse()")
        {
            return None;
        }

        if !line.contains(".ok().") && !line.contains(".ok()?") {
            return Some((
                "medium",
                "ok_swallow",
                ".ok() may be swallowing important errors",
            ));
        }
    }

    // Low severity: ignoring send errors on channels
    if line.contains("let _ =") && line.contains(".send(") && !line.contains("// ") {
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

    // Filter operations
    if line.contains("filter_map") || line.contains("filter(|") {
        return true;
    }

    // .ok() with explicit fallback handling
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

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════════
    // is_cfg_test
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_cfg_test_simple() {
        assert!(is_cfg_test("#[cfg(test)]"));
    }

    #[test]
    fn test_cfg_test_with_whitespace() {
        assert!(is_cfg_test("  #[cfg(test)]  "));
    }

    #[test]
    fn test_cfg_not_test_is_production() {
        // not(test) means production-only code, should NOT be treated as test
        assert!(!is_cfg_test("#[cfg(not(test))]"));
    }

    #[test]
    fn test_cfg_test_any_with_test() {
        assert!(is_cfg_test("#[cfg(any(test, feature = \"foo\"))]"));
    }

    #[test]
    fn test_cfg_test_all_with_test() {
        assert!(is_cfg_test("#[cfg(all(test, target_os = \"linux\"))]"));
    }

    #[test]
    fn test_cfg_not_test() {
        assert!(!is_cfg_test("#[cfg(feature = \"serde\")]"));
    }

    #[test]
    fn test_cfg_test_no_cfg() {
        assert!(!is_cfg_test("fn main() {}"));
    }

    #[test]
    fn test_cfg_test_empty() {
        assert!(!is_cfg_test(""));
    }

    #[test]
    fn test_cfg_test_partial_word() {
        // "testing" contains "test" but is a different word
        assert!(!is_cfg_test("#[cfg(feature = \"testing\")]"));
    }

    #[test]
    fn test_cfg_composite_not_test() {
        // test only appears under not() — this is production-only code
        assert!(!is_cfg_test("#[cfg(all(unix, not(test)))]"));
        assert!(!is_cfg_test(
            "#[cfg(any(target_os = \"linux\", not(test)))]"
        ));
    }

    #[test]
    fn test_cfg_composite_positive_and_negated_test() {
        // test appears both positively and negated — still test-gated
        assert!(is_cfg_test("#[cfg(any(test, not(test)))]"));
    }

    #[test]
    fn test_cfg_not_with_whitespace() {
        // not<whitespace>(test) is valid Rust — still production-only
        assert!(!is_cfg_test("#[cfg(not (test))]"));
        assert!(!is_cfg_test("#[cfg(all(unix, not (test)))]"));
        assert!(!is_cfg_test("#[cfg(not\t(test))]"));
        assert!(!is_cfg_test("#[cfg(not\n(test))]"));
    }

    #[test]
    fn test_cfg_feature_test_is_not_test() {
        // feature = "test" is a cargo feature, not the test cfg
        assert!(!is_cfg_test("#[cfg(feature = \"test\")]"));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // is_safe_unwrap
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_safe_unwrap_regex() {
        assert!(is_safe_unwrap("    Regex::new(r\"pattern\").unwrap()"));
    }

    #[test]
    fn test_safe_unwrap_mutex_lock() {
        assert!(is_safe_unwrap("    let guard = data.lock().unwrap();"));
    }

    #[test]
    fn test_safe_unwrap_rwlock_read() {
        assert!(is_safe_unwrap("    let r = rw.read().unwrap();"));
    }

    #[test]
    fn test_safe_unwrap_rwlock_write() {
        assert!(is_safe_unwrap("    let w = rw.write().unwrap();"));
    }

    #[test]
    fn test_safe_unwrap_channel_send() {
        assert!(is_safe_unwrap("    tx.send(msg).unwrap();"));
    }

    #[test]
    fn test_safe_unwrap_selector_parse() {
        assert!(is_safe_unwrap("    Selector::parse(\"div\").unwrap()"));
    }

    #[test]
    fn test_safe_unwrap_set_language() {
        assert!(is_safe_unwrap("    parser.set_language(lang).unwrap()"));
    }

    #[test]
    fn test_safe_unwrap_string_literal_at_end() {
        // Pattern checks for ".unwrap()" — quote immediately before .unwrap()
        assert!(is_safe_unwrap(r#"    contains(".unwrap()")"#));
    }

    #[test]
    fn test_safe_unwrap_string_literal_mid_string_not_caught() {
        // .unwrap() in middle of string literal doesn't match the heuristic
        assert!(!is_safe_unwrap(r#"    println!("call .unwrap() here")"#));
    }

    #[test]
    fn test_unsafe_unwrap_plain() {
        assert!(!is_safe_unwrap("    result.unwrap()"));
    }

    #[test]
    fn test_unsafe_unwrap_option() {
        assert!(!is_safe_unwrap("    some_option.unwrap()"));
    }

    #[test]
    fn test_safe_unwrap_mutex_expect() {
        assert!(is_safe_unwrap(
            "    let guard = data.lock().expect(\"poisoned\");"
        ));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // check_error_pattern
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_error_silent_db_execute() {
        let r = check_error_pattern("let _ = conn.execute(\"DELETE FROM foo\", []);");
        assert!(r.is_some());
        let (sev, pat, _) = r.unwrap();
        assert_eq!(sev, "high");
        assert_eq!(pat, "silent_db");
    }

    #[test]
    fn test_error_silent_db_insert() {
        assert!(check_error_pattern("let _ = conn.insert(data);").is_some());
    }

    #[test]
    fn test_error_silent_db_update() {
        assert!(check_error_pattern("let _ = db.update(row);").is_some());
    }

    #[test]
    fn test_error_silent_db_delete() {
        assert!(check_error_pattern("let _ = db.delete(id);").is_some());
    }

    #[test]
    fn test_error_ok_swallow() {
        let r = check_error_pattern("    let val = something.ok()");
        assert!(r.is_some());
        let (sev, pat, _) = r.unwrap();
        assert_eq!(sev, "medium");
        assert_eq!(pat, "ok_swallow");
    }

    #[test]
    fn test_error_ok_propagation_not_flagged() {
        // .ok()? is fine
        assert!(check_error_pattern("    something.ok()?").is_none());
    }

    #[test]
    fn test_error_ok_env_var_not_flagged() {
        assert!(check_error_pattern("    env::var(\"HOME\").ok()").is_none());
    }

    #[test]
    fn test_error_ok_parse_not_flagged() {
        assert!(check_error_pattern("    \"42\".parse::<i32>().ok()").is_none());
    }

    #[test]
    fn test_error_ok_chained_not_flagged() {
        // .ok().map(...) is method chaining, not bare .ok()
        assert!(check_error_pattern("    val.ok().map(|x| x + 1)").is_none());
    }

    #[test]
    fn test_error_send_ignore() {
        let r = check_error_pattern("let _ = tx.send(msg)");
        assert!(r.is_some());
        let (sev, pat, _) = r.unwrap();
        assert_eq!(sev, "low");
        assert_eq!(pat, "send_ignore");
    }

    #[test]
    fn test_error_send_ignore_with_comment_not_flagged() {
        assert!(check_error_pattern("let _ = tx.send(msg) // intentional").is_none());
    }

    #[test]
    fn test_error_no_pattern() {
        assert!(check_error_pattern("    let x = foo.bar();").is_none());
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // is_acceptable_error_swallow
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_acceptable_logging_error() {
        assert!(is_acceptable_error_swallow("    error!(\"failed\")"));
    }

    #[test]
    fn test_acceptable_logging_warn() {
        assert!(is_acceptable_error_swallow("    warn!(\"oops\")"));
    }

    #[test]
    fn test_acceptable_tracing() {
        assert!(is_acceptable_error_swallow(
            "    tracing::warn!(\"failed\")"
        ));
    }

    #[test]
    fn test_acceptable_intentional_comment() {
        assert!(is_acceptable_error_swallow("    .ok() // intentional"));
    }

    #[test]
    fn test_acceptable_ignore_comment() {
        assert!(is_acceptable_error_swallow("    let _ = x; // ignore"));
    }

    #[test]
    fn test_acceptable_ok_to_fail_comment() {
        assert!(is_acceptable_error_swallow("    let _ = x; // ok to fail"));
    }

    #[test]
    fn test_acceptable_filter_map() {
        assert!(is_acceptable_error_swallow(
            "    results.filter_map(|r| r.ok())"
        ));
    }

    #[test]
    fn test_acceptable_ok_unwrap_or() {
        assert!(is_acceptable_error_swallow("    val.ok().unwrap_or(0)"));
    }

    #[test]
    fn test_acceptable_ok_map() {
        assert!(is_acceptable_error_swallow("    val.ok().map(|x| x + 1)"));
    }

    #[test]
    fn test_acceptable_ok_and_then() {
        assert!(is_acceptable_error_swallow(
            "    val.ok().and_then(|x| Some(x))"
        ));
    }

    #[test]
    fn test_acceptable_ok_flatten() {
        assert!(is_acceptable_error_swallow("    val.ok().flatten()"));
    }

    #[test]
    fn test_acceptable_db_get_ok() {
        assert!(is_acceptable_error_swallow("    db.get_project(id).ok()"));
    }

    #[test]
    fn test_not_acceptable_bare_discard() {
        assert!(!is_acceptable_error_swallow("    let _ = conn.execute(q)"));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // DetectionResults::all_maxed
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_all_maxed_true() {
        let r = DetectionResults {
            todos: MAX_TODO_FINDINGS,
            unimplemented: MAX_UNIMPLEMENTED_FINDINGS,
            unwraps: MAX_UNWRAP_FINDINGS,
            error_handling: MAX_ERROR_HANDLING_FINDINGS,
        };
        assert!(r.all_maxed());
    }

    #[test]
    fn test_all_maxed_false_one_short() {
        let r = DetectionResults {
            todos: MAX_TODO_FINDINGS - 1,
            unimplemented: MAX_UNIMPLEMENTED_FINDINGS,
            unwraps: MAX_UNWRAP_FINDINGS,
            error_handling: MAX_ERROR_HANDLING_FINDINGS,
        };
        assert!(!r.all_maxed());
    }

    #[test]
    fn test_all_maxed_false_zeros() {
        let r = DetectionResults {
            todos: 0,
            unimplemented: 0,
            unwraps: 0,
            error_handling: 0,
        };
        assert!(!r.all_maxed());
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // store_detection_findings
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_store_findings_empty() {
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/det", Some("test")).unwrap();
        let count = store_detection_findings(&conn, pid, &[]).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_store_findings_persists() {
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/det", Some("test")).unwrap();

        let findings = vec![
            DetectionFinding {
                key: "health:todo:src/main.rs:10".to_string(),
                content: "[todo] src/main.rs:10 - TODO: fix this".to_string(),
                category: "todo",
                confidence: CONFIDENCE_TODO,
            },
            DetectionFinding {
                key: "health:unwrap:src/lib.rs:5".to_string(),
                content: "[high] .unwrap() at src/lib.rs:5".to_string(),
                category: "unwrap",
                confidence: CONFIDENCE_UNWRAP_HIGH,
            },
        ];

        let count = store_detection_findings(&conn, pid, &findings).unwrap();
        assert_eq!(count, 2);

        let stored: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_facts WHERE project_id = ? AND fact_type = 'health'",
                [pid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, 2);
    }
}
