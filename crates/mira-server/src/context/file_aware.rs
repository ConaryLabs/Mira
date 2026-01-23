// crates/mira-server/src/context/file_aware.rs
// File mention detection and context injection

use crate::db::Database;
use regex::Regex;
use std::sync::Arc;

pub struct FileAwareInjector {
    db: Arc<Database>,
    file_pattern: Regex,
}

impl FileAwareInjector {
    pub fn new(db: Arc<Database>) -> Self {
        // Match file paths with common extensions
        // Handles: src/main.rs, ./config.toml, /absolute/path.json, crates/foo/bar.rs
        let file_pattern = Regex::new(
            r#"(?:^|[\s`'"(])(\.?/?(?:[\w-]+/)*[\w-]+\.(?:rs|toml|json|md|txt|py|js|ts|tsx|jsx|go|yaml|yml|sh|sql|html|css|scss|vue|svelte))\b"#
        ).expect("Invalid regex");

        Self { db, file_pattern }
    }

    /// Extract file paths from user message using regex
    pub fn extract_file_mentions(&self, user_message: &str) -> Vec<String> {
        let mut paths: Vec<String> = self.file_pattern
            .captures_iter(user_message)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            .collect();

        // Deduplicate
        paths.sort();
        paths.dedup();

        paths
    }

    /// Inject context related to specific file paths
    /// Searches memories for mentions of these files
    pub async fn inject_file_context(&self, file_paths: Vec<String>) -> String {
        if file_paths.is_empty() {
            return String::new();
        }

        let mut context_parts = Vec::new();

        for path in &file_paths {
            // Extract just the filename for broader matching
            let filename = path.rsplit('/').next().unwrap_or(path);

            // Search memories that mention this file
            if let Ok(memories) = self.db.search_memories(None, filename, 3) {
                for mem in memories {
                    // Skip if it's just a health alert or low confidence
                    if mem.fact_type == "health" || mem.confidence < 0.5 {
                        continue;
                    }

                    // Format: [category] content (truncated)
                    let category = mem.category.as_deref().unwrap_or("note");
                    let content = if mem.content.len() > 150 {
                        format!("{}...", &mem.content[..150])
                    } else {
                        mem.content.clone()
                    };

                    context_parts.push(format!("[{}] {}", category, content));
                }
            }
        }

        if context_parts.is_empty() {
            return String::new();
        }

        // Deduplicate context parts
        context_parts.sort();
        context_parts.dedup();

        // Limit to 3 most relevant
        context_parts.truncate(3);

        format!(
            "Related to files mentioned ({}):\n{}",
            file_paths.join(", "),
            context_parts.join("\n")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_injector() -> FileAwareInjector {
        let db = Arc::new(Database::open_in_memory().unwrap());
        FileAwareInjector::new(db)
    }

    #[test]
    fn test_extract_rust_files() {
        let injector = create_test_injector();

        let paths = injector.extract_file_mentions("Look at src/main.rs and lib.rs");
        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(paths.contains(&"lib.rs".to_string()));
    }

    #[test]
    fn test_extract_nested_paths() {
        let injector = create_test_injector();

        let paths = injector.extract_file_mentions("Check crates/mira-server/src/db/pool.rs");
        assert!(paths.contains(&"crates/mira-server/src/db/pool.rs".to_string()));
    }

    #[test]
    fn test_extract_various_extensions() {
        let injector = create_test_injector();

        let msg = "Edit config.toml, schema.json, and README.md";
        let paths = injector.extract_file_mentions(msg);

        assert!(paths.contains(&"config.toml".to_string()));
        assert!(paths.contains(&"schema.json".to_string()));
        assert!(paths.contains(&"README.md".to_string()));
    }

    #[test]
    fn test_extract_relative_paths() {
        let injector = create_test_injector();

        let paths = injector.extract_file_mentions("Run ./scripts/build.sh");
        assert!(paths.contains(&"./scripts/build.sh".to_string()));
    }

    #[test]
    fn test_no_duplicates() {
        let injector = create_test_injector();

        let paths = injector.extract_file_mentions("main.rs and main.rs again");
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn test_paths_in_backticks() {
        let injector = create_test_injector();

        let paths = injector.extract_file_mentions("Check `src/lib.rs` for the implementation");
        assert!(paths.contains(&"src/lib.rs".to_string()));
    }
}
