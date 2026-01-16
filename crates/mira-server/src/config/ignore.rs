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
    "env",
    "assets",
    "coverage",
];

/// Python-specific skip directories
pub const PYTHON_SKIP: &[&str] = &[".egg-info", ".tox", ".pytest_cache"];

/// Node-specific skip directories
pub const NODE_SKIP: &[&str] = &["__tests__", "__mocks__", ".turbo"];

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
