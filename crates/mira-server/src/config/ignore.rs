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

    #[test]
    fn test_load_project_ignore_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let ignore_content = "\n# comment\nmy_dir\n\nanother_dir\n";
        fs::write(temp_dir.path().join(".miraignore"), ignore_content).unwrap();
        let patterns = load_project_ignore_patterns(temp_dir.path());
        assert_eq!(patterns, vec!["my_dir", "another_dir"]);
    }

    #[test]
    fn test_should_skip_with_patterns() {
        let extra = vec!["my_custom_dir".to_string()];
        assert!(should_skip_with_patterns("my_custom_dir", &extra));
        assert!(should_skip_with_patterns("node_modules", &extra));
        assert!(should_skip_with_patterns(".git", &extra));
        assert!(!should_skip_with_patterns("src", &extra));
    }

    #[test]
    fn test_should_skip_for_lang_with_patterns() {
        let extra = vec!["my_custom_dir".to_string()];
        assert!(should_skip_for_lang_with_patterns("my_custom_dir", "python", &extra));
        assert!(should_skip_for_lang_with_patterns(".pytest_cache", "python", &extra));
        assert!(!should_skip_for_lang_with_patterns("src", "python", &extra));
    }
}
