// crates/mira-server/src/context/file_aware.rs
// File mention detection and context injection

use crate::db::pool::DatabasePool;
use regex::Regex;
use std::sync::Arc;

pub struct FileAwareInjector {
    #[allow(dead_code)]
    pool: Arc<DatabasePool>,
    file_pattern: Regex,
}

impl FileAwareInjector {
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        // Match file paths with common extensions
        // Handles: src/main.rs, ./config.toml, /absolute/path.json, crates/foo/bar.rs
        #[allow(clippy::expect_used)] // Constant regex pattern - compile-time validated
        let file_pattern = Regex::new(
            r#"(?:^|[\s`'"(])(\.?/?(?:[\w-]+/)*[\w-]+\.(?:rs|toml|json|md|txt|py|js|ts|tsx|jsx|go|yaml|yml|sh|sql|html|css|scss|vue|svelte))\b"#
        ).expect("Invalid regex");

        Self { pool, file_pattern }
    }

    /// Extract file paths from user message using regex
    pub fn extract_file_mentions(&self, user_message: &str) -> Vec<String> {
        let mut paths: Vec<String> = self
            .file_pattern
            .captures_iter(user_message)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            .collect();

        // Deduplicate
        paths.sort();
        paths.dedup();

        paths
    }

    /// Inject context related to specific file paths.
    /// Memory system removed (Phase 4) -- returns empty string.
    pub async fn inject_file_context(&self, file_paths: Vec<String>) -> String {
        if file_paths.is_empty() {
            return String::new();
        }

        // Memory-based file context removed in Phase 4 of memory system removal.
        // File tracking is now handled by observations and code index.
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_injector() -> FileAwareInjector {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        FileAwareInjector::new(pool)
    }

    #[tokio::test]
    async fn test_extract_rust_files() {
        let injector = create_test_injector().await;

        let paths = injector.extract_file_mentions("Look at src/main.rs and lib.rs");
        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(paths.contains(&"lib.rs".to_string()));
    }

    #[tokio::test]
    async fn test_extract_nested_paths() {
        let injector = create_test_injector().await;

        let paths = injector.extract_file_mentions("Check crates/mira-server/src/db/pool.rs");
        assert!(paths.contains(&"crates/mira-server/src/db/pool.rs".to_string()));
    }

    #[tokio::test]
    async fn test_extract_various_extensions() {
        let injector = create_test_injector().await;

        let msg = "Edit config.toml, schema.json, and README.md";
        let paths = injector.extract_file_mentions(msg);

        assert!(paths.contains(&"config.toml".to_string()));
        assert!(paths.contains(&"schema.json".to_string()));
        assert!(paths.contains(&"README.md".to_string()));
    }

    #[tokio::test]
    async fn test_extract_relative_paths() {
        let injector = create_test_injector().await;

        let paths = injector.extract_file_mentions("Run ./scripts/build.sh");
        assert!(paths.contains(&"./scripts/build.sh".to_string()));
    }

    #[tokio::test]
    async fn test_no_duplicates() {
        let injector = create_test_injector().await;

        let paths = injector.extract_file_mentions("main.rs and main.rs again");
        assert_eq!(paths.len(), 1);
    }

    #[tokio::test]
    async fn test_paths_in_backticks() {
        let injector = create_test_injector().await;

        let paths = injector.extract_file_mentions("Check `src/lib.rs` for the implementation");
        assert!(paths.contains(&"src/lib.rs".to_string()));
    }
}
