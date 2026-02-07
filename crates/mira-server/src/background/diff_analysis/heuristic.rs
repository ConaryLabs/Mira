// background/diff_analysis/heuristic.rs
// Heuristic (non-LLM) diff analysis and risk calculation

use super::types::{DiffStats, SemanticChange};

/// Security-relevant keywords for heuristic scanning
const SECURITY_KEYWORDS: &[&str] = &[
    "password",
    "token",
    "secret",
    "auth",
    "sql",
    "unsafe",
    "exec",
    "eval",
    "credential",
    "private_key",
    "api_key",
    "encrypt",
    "decrypt",
    "hash",
    "permission",
    "privilege",
    "sanitize",
    "injection",
];

/// Function definition patterns for heuristic detection
const FUNCTION_PATTERNS: &[&str] = &["fn ", "def ", "function ", "class ", "impl "];

/// Analyze diff heuristically without LLM
pub fn analyze_diff_heuristic(
    diff_content: &str,
    stats: &DiffStats,
) -> (Vec<SemanticChange>, String, Vec<String>) {
    if diff_content.is_empty() {
        return (Vec::new(), "[heuristic] No changes".to_string(), Vec::new());
    }

    let mut changes = Vec::new();
    let mut risk_flags = Vec::new();
    let mut current_file: Option<String> = None;
    let mut security_hits: Vec<String> = Vec::new();

    for line in diff_content.lines() {
        // Parse file headers: "diff --git a/path b/path"
        if line.starts_with("diff --git ") {
            // Extract file path from "diff --git a/foo b/foo"
            if let Some(b_part) = line.split(" b/").last() {
                current_file = Some(b_part.to_string());
            }
            continue;
        }

        // Handle rename lines: "rename from ..." / "rename to ..."
        if line.starts_with("rename to ") {
            if let Some(path) = line.strip_prefix("rename to ") {
                current_file = Some(path.to_string());
            }
            continue;
        }

        // Skip binary diffs
        if line.starts_with("Binary files") {
            continue;
        }

        // Only scan added/removed lines within hunks
        let is_added = line.starts_with('+') && !line.starts_with("+++");
        let is_removed = line.starts_with('-') && !line.starts_with("---");

        if !is_added && !is_removed {
            continue;
        }

        let content = &line[1..];
        let file_path = current_file.as_deref().unwrap_or("");

        // Detect function definitions in changed lines
        for pattern in FUNCTION_PATTERNS {
            if content.contains(pattern) {
                let symbol_name = extract_symbol_name(content, pattern);
                let change_type = if is_added {
                    "NewFunction"
                } else {
                    "DeletedFunction"
                };
                // Avoid duplicates for the same symbol in the same file
                let already_exists = changes.iter().any(|c: &SemanticChange| {
                    c.file_path == file_path
                        && c.symbol_name.as_deref() == Some(symbol_name.as_str())
                        && c.change_type == change_type
                });
                if !already_exists {
                    changes.push(SemanticChange {
                        change_type: change_type.to_string(),
                        file_path: file_path.to_string(),
                        symbol_name: Some(symbol_name),
                        description: format!(
                            "{} {}",
                            if is_added { "Added" } else { "Removed" },
                            pattern.trim()
                        ),
                        breaking: is_removed,
                        security_relevant: false,
                    });
                }
                break;
            }
        }

        // Scan for security-relevant keywords
        let lower = content.to_lowercase();
        for keyword in SECURITY_KEYWORDS {
            if lower.contains(keyword) {
                security_hits.push(format!("{}:{}", file_path, keyword));
                break;
            }
        }
    }

    // Mark security-relevant changes
    if !security_hits.is_empty() {
        risk_flags.push("security_relevant_change".to_string());
        // Mark changes in files with security hits as security_relevant
        let security_files: std::collections::HashSet<String> = security_hits
            .iter()
            .filter_map(|h| h.split(':').next().map(|s| s.to_string()))
            .collect();
        for change in &mut changes {
            if security_files.contains(&change.file_path) {
                change.security_relevant = true;
            }
        }
    }

    // Risk flag: large change (>500 total lines)
    if stats.lines_added + stats.lines_removed > 500 {
        risk_flags.push("large_change".to_string());
    }

    // Risk flag: wide change (>10 files)
    if stats.files_changed > 10 {
        risk_flags.push("wide_change".to_string());
    }

    // Risk flag: breaking API change (removed functions)
    let removed_count = changes
        .iter()
        .filter(|c| c.change_type == "DeletedFunction")
        .count();
    if removed_count > 0 {
        risk_flags.push("breaking_api_change".to_string());
    }

    // Build summary
    let added_fns = changes
        .iter()
        .filter(|c| c.change_type == "NewFunction")
        .count();
    let summary = format!(
        "[heuristic] {} files changed (+{} -{}), {} functions added, {} removed{}",
        stats.files_changed,
        stats.lines_added,
        stats.lines_removed,
        added_fns,
        removed_count,
        if !security_hits.is_empty() {
            format!("; {} security-relevant change(s)", security_hits.len())
        } else {
            String::new()
        }
    );

    (changes, summary, risk_flags)
}

/// Extract a symbol name from a line containing a function/class pattern
fn extract_symbol_name(line: &str, pattern: &str) -> String {
    if let Some(after) = line.split(pattern).nth(1) {
        // Take everything up to first ( or { or : or < or whitespace
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            return name;
        }
    }
    "unknown".to_string()
}

/// Calculate overall risk level from flags
pub fn calculate_risk_level(flags: &[String], changes: &[SemanticChange]) -> String {
    let has_breaking = changes.iter().any(|c| c.breaking);
    let has_security = changes.iter().any(|c| c.security_relevant);
    let breaking_count = flags.iter().filter(|f| f.contains("breaking")).count();
    let security_count = flags.iter().filter(|f| f.contains("security")).count();

    if has_security || security_count > 0 {
        if has_breaking || breaking_count > 0 {
            return "Critical".to_string();
        }
        return "High".to_string();
    }

    if has_breaking || breaking_count > 1 {
        return "High".to_string();
    }

    if breaking_count > 0 || flags.len() > 3 {
        return "Medium".to_string();
    }

    "Low".to_string()
}
