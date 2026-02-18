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

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // extract_symbol_name Tests
    // =========================================================================

    #[test]
    fn test_extract_symbol_name_rust_fn() {
        let result = extract_symbol_name(
            "    pub fn process_batch(items: &[Item]) -> Result<()>",
            "fn ",
        );
        assert_eq!(result, "process_batch");
    }

    #[test]
    fn test_extract_symbol_name_python_def() {
        let result = extract_symbol_name("def handle_request(self, req):", "def ");
        assert_eq!(result, "handle_request");
    }

    #[test]
    fn test_extract_symbol_name_class() {
        let result = extract_symbol_name("class DatabasePool {", "class ");
        assert_eq!(result, "DatabasePool");
    }

    #[test]
    fn test_extract_symbol_name_impl_trait() {
        let result = extract_symbol_name("impl Display for Config {", "impl ");
        assert_eq!(result, "Display");
    }

    #[test]
    fn test_extract_symbol_name_with_underscores() {
        let result = extract_symbol_name("fn my_long_function_name()", "fn ");
        assert_eq!(result, "my_long_function_name");
    }

    #[test]
    fn test_extract_symbol_name_no_match_returns_unknown() {
        // Pattern present but nothing alphanumeric follows
        let result = extract_symbol_name("fn ()", "fn ");
        assert_eq!(result, "unknown");
    }

    #[test]
    fn test_extract_symbol_name_pattern_not_found() {
        let result = extract_symbol_name("let x = 5;", "fn ");
        assert_eq!(result, "unknown");
    }

    // =========================================================================
    // analyze_diff_heuristic Tests
    // =========================================================================

    #[test]
    fn test_analyze_diff_heuristic_empty_diff() {
        let stats = DiffStats::default();
        let (changes, summary, risk_flags) = analyze_diff_heuristic("", &stats);

        assert!(changes.is_empty(), "Empty diff should produce no changes");
        assert_eq!(summary, "[heuristic] No changes");
        assert!(
            risk_flags.is_empty(),
            "Empty diff should produce no risk flags"
        );
    }

    #[test]
    fn test_analyze_diff_heuristic_added_function() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,7 @@
+pub fn new_feature(input: &str) -> bool {
+    true
+}";
        let stats = DiffStats {
            files_changed: 1,
            lines_added: 3,
            lines_removed: 0,
            files: vec!["src/lib.rs".to_string()],
        };

        let (changes, summary, _risk_flags) = analyze_diff_heuristic(diff, &stats);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change_type, "NewFunction");
        assert_eq!(changes[0].symbol_name.as_deref(), Some("new_feature"));
        assert_eq!(changes[0].file_path, "src/lib.rs");
        assert!(
            !changes[0].breaking,
            "Added function should not be breaking"
        );
        assert!(summary.contains("1 functions added"));
    }

    #[test]
    fn test_analyze_diff_heuristic_removed_function_is_breaking() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,0 @@
-pub fn old_api(x: i32) -> String {
-    x.to_string()
-}";
        let stats = DiffStats {
            files_changed: 1,
            lines_added: 0,
            lines_removed: 3,
            files: vec!["src/lib.rs".to_string()],
        };

        let (changes, _summary, risk_flags) = analyze_diff_heuristic(diff, &stats);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change_type, "DeletedFunction");
        assert!(
            changes[0].breaking,
            "Removed function should be marked breaking"
        );
        assert!(
            risk_flags.contains(&"breaking_api_change".to_string()),
            "Should flag breaking API change"
        );
    }

    #[test]
    fn test_analyze_diff_heuristic_security_keywords_detected() {
        let diff = "\
diff --git a/src/auth.rs b/src/auth.rs
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -1,3 +1,5 @@
+fn validate_password(input: &str) -> bool {
+    let token = generate_auth_token();
+}";
        let stats = DiffStats {
            files_changed: 1,
            lines_added: 3,
            lines_removed: 0,
            files: vec!["src/auth.rs".to_string()],
        };

        let (changes, summary, risk_flags) = analyze_diff_heuristic(diff, &stats);

        assert!(
            risk_flags.contains(&"security_relevant_change".to_string()),
            "Should detect security-relevant keywords"
        );
        assert!(summary.contains("security-relevant change"));
        // The function change in the security file should be marked security_relevant
        let security_changes: Vec<_> = changes.iter().filter(|c| c.security_relevant).collect();
        assert!(
            !security_changes.is_empty(),
            "Changes in files with security keywords should be marked security_relevant"
        );
    }

    #[test]
    fn test_analyze_diff_heuristic_large_change_risk_flag() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
+some line";
        let stats = DiffStats {
            files_changed: 1,
            lines_added: 400,
            lines_removed: 200,
            files: vec!["src/lib.rs".to_string()],
        };

        let (_changes, _summary, risk_flags) = analyze_diff_heuristic(diff, &stats);

        assert!(
            risk_flags.contains(&"large_change".to_string()),
            "Should flag changes exceeding 500 total lines"
        );
    }

    #[test]
    fn test_analyze_diff_heuristic_wide_change_risk_flag() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
+some line";
        let stats = DiffStats {
            files_changed: 15,
            lines_added: 10,
            lines_removed: 5,
            files: vec!["src/lib.rs".to_string()],
        };

        let (_changes, _summary, risk_flags) = analyze_diff_heuristic(diff, &stats);

        assert!(
            risk_flags.contains(&"wide_change".to_string()),
            "Should flag changes affecting >10 files"
        );
    }

    #[test]
    fn test_analyze_diff_heuristic_binary_files_skipped() {
        let diff = "\
diff --git a/image.png b/image.png
Binary files a/image.png and b/image.png differ";
        let stats = DiffStats {
            files_changed: 1,
            lines_added: 0,
            lines_removed: 0,
            files: vec!["image.png".to_string()],
        };

        let (changes, _summary, risk_flags) = analyze_diff_heuristic(diff, &stats);

        assert!(
            changes.is_empty(),
            "Binary file diffs should produce no changes"
        );
        assert!(
            !risk_flags.contains(&"security_relevant_change".to_string()),
            "Binary files should not trigger security scanning"
        );
    }

    #[test]
    fn test_analyze_diff_heuristic_rename_handling() {
        let diff = "\
diff --git a/old_name.rs b/new_name.rs
rename from old_name.rs
rename to new_name.rs
+pub fn renamed_fn() {}";
        let stats = DiffStats {
            files_changed: 1,
            lines_added: 1,
            lines_removed: 0,
            files: vec!["new_name.rs".to_string()],
        };

        let (changes, _summary, _risk_flags) = analyze_diff_heuristic(diff, &stats);

        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0].file_path, "new_name.rs",
            "After rename, file_path should reflect the new name"
        );
    }

    #[test]
    fn test_analyze_diff_heuristic_python_and_class_detection() {
        let diff = "\
diff --git a/app.py b/app.py
+class UserService:
+    def authenticate(self, user):
+        pass";
        let stats = DiffStats {
            files_changed: 1,
            lines_added: 3,
            lines_removed: 0,
            files: vec!["app.py".to_string()],
        };

        let (changes, _summary, _risk_flags) = analyze_diff_heuristic(diff, &stats);

        let class_changes: Vec<_> = changes
            .iter()
            .filter(|c| c.symbol_name.as_deref() == Some("UserService"))
            .collect();
        assert_eq!(class_changes.len(), 1, "Should detect class definition");

        let def_changes: Vec<_> = changes
            .iter()
            .filter(|c| c.symbol_name.as_deref() == Some("authenticate"))
            .collect();
        assert_eq!(def_changes.len(), 1, "Should detect def definition");
    }

    #[test]
    fn test_analyze_diff_heuristic_diff_header_lines_not_scanned() {
        // +++ and --- lines should be skipped (they are file headers, not content)
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 context line";
        let stats = DiffStats {
            files_changed: 1,
            lines_added: 0,
            lines_removed: 0,
            files: vec!["src/lib.rs".to_string()],
        };

        let (changes, _summary, _risk_flags) = analyze_diff_heuristic(diff, &stats);

        assert!(
            changes.is_empty(),
            "+++ and --- header lines should not be treated as added/removed content"
        );
    }

    #[test]
    fn test_analyze_diff_heuristic_summary_format() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
+pub fn added_fn() {}
-pub fn removed_fn() {}";
        let stats = DiffStats {
            files_changed: 2,
            lines_added: 10,
            lines_removed: 5,
            files: vec!["src/lib.rs".to_string()],
        };

        let (_changes, summary, _risk_flags) = analyze_diff_heuristic(diff, &stats);

        assert!(
            summary.starts_with("[heuristic]"),
            "Summary should start with [heuristic] prefix"
        );
        assert!(
            summary.contains("2 files changed"),
            "Summary should contain files_changed from stats"
        );
        assert!(
            summary.contains("+10"),
            "Summary should contain lines_added from stats"
        );
        assert!(
            summary.contains("-5"),
            "Summary should contain lines_removed from stats"
        );
    }

    #[test]
    fn test_analyze_diff_heuristic_deduplicates_same_symbol() {
        // Same function added twice in different hunks of the same file
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
+pub fn duplicate_fn() {}
+pub fn duplicate_fn() {}";
        let stats = DiffStats::default();

        let (changes, _summary, _risk_flags) = analyze_diff_heuristic(diff, &stats);

        let duplicate_changes: Vec<_> = changes
            .iter()
            .filter(|c| c.symbol_name.as_deref() == Some("duplicate_fn"))
            .collect();
        assert_eq!(
            duplicate_changes.len(),
            1,
            "Should deduplicate same symbol in same file"
        );
    }

    // =========================================================================
    // calculate_risk_level Tests
    // =========================================================================

    #[test]
    fn test_calculate_risk_level_no_flags_no_changes() {
        let flags: Vec<String> = vec![];
        let changes: Vec<SemanticChange> = vec![];
        assert_eq!(calculate_risk_level(&flags, &changes), "Low");
    }

    #[test]
    fn test_calculate_risk_level_security_relevant_is_high() {
        let flags = vec!["security_relevant_change".to_string()];
        let changes: Vec<SemanticChange> = vec![];
        assert_eq!(calculate_risk_level(&flags, &changes), "High");
    }

    #[test]
    fn test_calculate_risk_level_security_and_breaking_is_critical() {
        let flags = vec![
            "security_relevant_change".to_string(),
            "breaking_api_change".to_string(),
        ];
        let changes: Vec<SemanticChange> = vec![];
        assert_eq!(calculate_risk_level(&flags, &changes), "Critical");
    }

    #[test]
    fn test_calculate_risk_level_breaking_change_in_changes() {
        let flags: Vec<String> = vec![];
        let changes = vec![SemanticChange {
            change_type: "DeletedFunction".to_string(),
            file_path: "src/lib.rs".to_string(),
            symbol_name: Some("old_fn".to_string()),
            description: "Removed fn".to_string(),
            breaking: true,
            security_relevant: false,
        }];
        assert_eq!(calculate_risk_level(&flags, &changes), "High");
    }

    #[test]
    fn test_calculate_risk_level_security_change_object_is_high() {
        let flags: Vec<String> = vec![];
        let changes = vec![SemanticChange {
            change_type: "NewFunction".to_string(),
            file_path: "src/auth.rs".to_string(),
            symbol_name: Some("validate".to_string()),
            description: "Added fn".to_string(),
            breaking: false,
            security_relevant: true,
        }];
        assert_eq!(calculate_risk_level(&flags, &changes), "High");
    }

    #[test]
    fn test_calculate_risk_level_many_flags_is_medium() {
        let flags = vec![
            "flag1".to_string(),
            "flag2".to_string(),
            "flag3".to_string(),
            "flag4".to_string(),
        ];
        let changes: Vec<SemanticChange> = vec![];
        assert_eq!(calculate_risk_level(&flags, &changes), "Medium");
    }

    #[test]
    fn test_calculate_risk_level_single_breaking_flag_is_medium() {
        let flags = vec!["breaking_api_change".to_string()];
        let changes: Vec<SemanticChange> = vec![];
        assert_eq!(calculate_risk_level(&flags, &changes), "Medium");
    }
}
