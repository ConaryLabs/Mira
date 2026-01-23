// crates/mira-server/src/config/ignore.rs
// Centralized directory ignore lists

/// Common directories to skip across all languages
pub const COMMON_SKIP: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "pkg",
    "dist",
    "build",
    "vendor",
    "__pycache__",
    ".next",
    "out",
    ".venv",
    "venv",
    "coverage",
];

/// Python-specific skip directories
pub const PYTHON_SKIP: &[&str] = &[".egg-info", ".tox", ".pytest_cache", "env"];

/// Node-specific skip directories
pub const NODE_SKIP: &[&str] = &["__tests__", "__mocks__", ".turbo", "assets"];

/// Go-specific skip directories
pub const GO_SKIP: &[&str] = &["testdata"];

/// Check if directory should be skipped (common rules)
pub fn should_skip(name: &str) -> bool {
    name.starts_with('.') || COMMON_SKIP.contains(&name)
}

/// Check with language-specific rules
pub fn should_skip_for_lang(name: &str, lang: &str) -> bool {
    if should_skip(name) {
        return true;
    }
    match lang {
        "python" => PYTHON_SKIP.contains(&name),
        "node" | "typescript" | "javascript" => NODE_SKIP.contains(&name),
        "go" => GO_SKIP.contains(&name),
        _ => false,
    }
}

/// Load additional ignore patterns from .miraignore file in the given directory.
/// Returns a vector of pattern strings (directory names).
pub fn load_project_ignore_patterns(root: &std::path::Path) -> Vec<String> {
    use std::fs;
    use std::io::{self, BufRead};

    let ignore_file = root.join(".miraignore");
    if !ignore_file.exists() {
        return Vec::new();
    }

    match fs::File::open(&ignore_file) {
        Ok(file) => {
            let reader = io::BufReader::new(file);
            let mut patterns = Vec::new();
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        let line = line.trim();
                        // Skip empty lines and comments
                        if line.is_empty() || line.starts_with('#') {
                            continue;
                        }
                        patterns.push(line.to_string());
                    }
                    Err(_) => break,
                }
            }
            patterns
        }
        Err(_) => Vec::new(),
    }
}

/// Check if directory should be skipped with additional patterns.
pub fn should_skip_with_patterns(name: &str, extra_patterns: &[String]) -> bool {
    name.starts_with('.') || COMMON_SKIP.contains(&name) || extra_patterns.iter().any(|p| p == name)
}

/// Check with language-specific rules and additional patterns.
pub fn should_skip_for_lang_with_patterns(name: &str, lang: &str, extra_patterns: &[String]) -> bool {
    if should_skip_with_patterns(name, extra_patterns) {
        return true;
    }
    match lang {
        "python" => PYTHON_SKIP.contains(&name),
        "node" | "typescript" | "javascript" => NODE_SKIP.contains(&name),
        "go" => GO_SKIP.contains(&name),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ============================================================================
    // should_skip tests
    // ============================================================================

    #[test]
    fn test_should_skip_common_dirs() {
        assert!(should_skip("node_modules"));
        assert!(should_skip("target"));
        assert!(should_skip(".git"));
        assert!(should_skip("dist"));
        assert!(should_skip("build"));
        assert!(should_skip("__pycache__"));
        assert!(should_skip(".venv"));
    }

    #[test]
    fn test_should_skip_hidden_dirs() {
        assert!(should_skip(".hidden"));
        assert!(should_skip(".config"));
        assert!(should_skip(".cache"));
    }

    #[test]
    fn test_should_skip_source_dirs() {
        assert!(!should_skip("src"));
        assert!(!should_skip("lib"));
        assert!(!should_skip("crates"));
        assert!(!should_skip("packages"));
    }

    // ============================================================================
    // should_skip_for_lang tests
    // ============================================================================

    #[test]
    fn test_should_skip_for_lang_python() {
        assert!(should_skip_for_lang(".pytest_cache", "python"));
        assert!(should_skip_for_lang(".egg-info", "python"));
        assert!(should_skip_for_lang(".tox", "python"));
        assert!(should_skip_for_lang("env", "python"));
        assert!(!should_skip_for_lang("src", "python"));
    }

    #[test]
    fn test_should_skip_for_lang_node() {
        assert!(should_skip_for_lang("__tests__", "node"));
        assert!(should_skip_for_lang("__mocks__", "node"));
        assert!(should_skip_for_lang(".turbo", "node"));
        assert!(should_skip_for_lang("assets", "node"));
        assert!(!should_skip_for_lang("src", "node"));
    }

    #[test]
    fn test_should_skip_for_lang_typescript() {
        // typescript should use node rules
        assert!(should_skip_for_lang("__tests__", "typescript"));
        assert!(should_skip_for_lang("assets", "typescript"));
    }

    #[test]
    fn test_should_skip_for_lang_javascript() {
        // javascript should use node rules
        assert!(should_skip_for_lang("__tests__", "javascript"));
        assert!(should_skip_for_lang("assets", "javascript"));
    }

    #[test]
    fn test_should_skip_for_lang_go() {
        assert!(should_skip_for_lang("testdata", "go"));
        assert!(!should_skip_for_lang("cmd", "go"));
        assert!(!should_skip_for_lang("internal", "go"));
    }

    #[test]
    fn test_should_skip_for_lang_unknown() {
        // Unknown language should only skip common dirs and hidden dirs
        assert!(should_skip_for_lang("node_modules", "unknown"));
        assert!(should_skip_for_lang(".git", "unknown"));
        assert!(!should_skip_for_lang("__tests__", "unknown")); // node-specific, not skipped
        // .pytest_cache starts with '.', so it IS skipped by should_skip
        assert!(should_skip_for_lang(".pytest_cache", "unknown"));
        // But 'env' without dot would not be skipped for unknown lang
        assert!(!should_skip_for_lang("env", "unknown")); // python-specific
    }

    // ============================================================================
    // load_project_ignore_patterns tests
    // ============================================================================

    #[test]
    fn test_load_project_ignore_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let ignore_content = "\n# comment\nmy_dir\n\nanother_dir\n";
        fs::write(temp_dir.path().join(".miraignore"), ignore_content).unwrap();
        let patterns = load_project_ignore_patterns(temp_dir.path());
        assert_eq!(patterns, vec!["my_dir", "another_dir"]);
    }

    #[test]
    fn test_load_project_ignore_patterns_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let patterns = load_project_ignore_patterns(temp_dir.path());
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_load_project_ignore_patterns_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join(".miraignore"), "").unwrap();
        let patterns = load_project_ignore_patterns(temp_dir.path());
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_load_project_ignore_patterns_comments_only() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join(".miraignore"), "# comment 1\n# comment 2\n").unwrap();
        let patterns = load_project_ignore_patterns(temp_dir.path());
        assert!(patterns.is_empty());
    }

    // ============================================================================
    // should_skip_with_patterns tests
    // ============================================================================

    #[test]
    fn test_should_skip_with_patterns() {
        let extra = vec!["my_custom_dir".to_string()];
        assert!(should_skip_with_patterns("my_custom_dir", &extra));
        assert!(should_skip_with_patterns("node_modules", &extra));
        assert!(should_skip_with_patterns(".git", &extra));
        assert!(!should_skip_with_patterns("src", &extra));
    }

    #[test]
    fn test_should_skip_with_patterns_empty_extra() {
        let extra: Vec<String> = vec![];
        assert!(should_skip_with_patterns("node_modules", &extra));
        assert!(!should_skip_with_patterns("custom", &extra));
    }

    // ============================================================================
    // should_skip_for_lang_with_patterns tests
    // ============================================================================

    #[test]
    fn test_should_skip_for_lang_with_patterns() {
        let extra = vec!["my_custom_dir".to_string()];
        assert!(should_skip_for_lang_with_patterns("my_custom_dir", "python", &extra));
        assert!(should_skip_for_lang_with_patterns(".pytest_cache", "python", &extra));
        assert!(!should_skip_for_lang_with_patterns("src", "python", &extra));
    }

    #[test]
    fn test_should_skip_for_lang_with_patterns_combines_all() {
        let extra = vec!["custom".to_string()];
        // Custom pattern
        assert!(should_skip_for_lang_with_patterns("custom", "python", &extra));
        // Common pattern
        assert!(should_skip_for_lang_with_patterns("node_modules", "python", &extra));
        // Language-specific pattern
        assert!(should_skip_for_lang_with_patterns(".pytest_cache", "python", &extra));
        // None of the above
        assert!(!should_skip_for_lang_with_patterns("src", "python", &extra));
    }

    // ============================================================================
    // Constants tests
    // ============================================================================

    #[test]
    fn test_common_skip_contains_essentials() {
        assert!(COMMON_SKIP.contains(&"node_modules"));
        assert!(COMMON_SKIP.contains(&"target"));
        assert!(COMMON_SKIP.contains(&".git"));
        assert!(COMMON_SKIP.contains(&"__pycache__"));
        assert!(COMMON_SKIP.contains(&".venv"));
    }

    #[test]
    fn test_python_skip_is_python_specific() {
        assert!(PYTHON_SKIP.contains(&".egg-info"));
        assert!(PYTHON_SKIP.contains(&".tox"));
        assert!(PYTHON_SKIP.contains(&".pytest_cache"));
    }

    #[test]
    fn test_node_skip_is_node_specific() {
        assert!(NODE_SKIP.contains(&"__tests__"));
        assert!(NODE_SKIP.contains(&"__mocks__"));
    }

    #[test]
    fn test_go_skip_is_go_specific() {
        assert!(GO_SKIP.contains(&"testdata"));
    }
}
