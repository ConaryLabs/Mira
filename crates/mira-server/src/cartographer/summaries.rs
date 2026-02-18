// crates/mira-server/src/cartographer/summaries.rs
// LLM-powered module summaries

use super::types::ModuleSummaryContext;
use crate::db::{get_modules_needing_summaries_sync, update_module_purposes_sync};
use crate::project_files::FileWalker;
use crate::utils::truncate_at_boundary;
use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

/// Get modules that need LLM summaries (no purpose or heuristic-only)
pub fn get_modules_needing_summaries(
    conn: &Connection,
    project_id: i64,
) -> Result<Vec<ModuleSummaryContext>> {
    Ok(get_modules_needing_summaries_sync(conn, project_id)?)
}

/// Read code preview for a module (first ~50 lines of key files)
pub fn get_module_code_preview(project_path: &Path, module_path: &str) -> String {
    let full_path = project_path.join(module_path);
    let mut preview = String::new();

    // Try mod.rs first, then lib.rs, then main.rs
    let candidates = ["mod.rs", "lib.rs", "main.rs"];

    for candidate in candidates {
        let file_path = if full_path.is_dir() {
            full_path.join(candidate)
        } else if full_path.extension().is_some_and(|e| e == "rs") {
            full_path.clone()
        } else {
            continue;
        };

        if file_path.exists()
            && let Ok(content) = std::fs::read_to_string(&file_path)
        {
            // Take first 50 lines
            let lines: Vec<&str> = content.lines().take(50).collect();
            preview = lines.join("\n");
            break;
        }
    }

    // If still empty, try to find any .rs file in the directory
    if preview.is_empty() && full_path.is_dir() {
        for path in FileWalker::new(&full_path)
            .for_language("rust")
            .max_depth(1)
            .walk_paths()
            .filter_map(|p| p.ok())
        {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let lines: Vec<&str> = content.lines().take(50).collect();
                preview = lines.join("\n");
                break;
            }
        }
    }

    preview
}

/// Read full code for a module (all .rs files in the directory)
/// Returns concatenated code with file headers, up to max_bytes total
pub fn get_module_full_code(project_path: &Path, module_path: &str, max_bytes: usize) -> String {
    let full_path = project_path.join(module_path);
    let mut code = String::new();
    let mut total_bytes = 0;

    // Collect all .rs files in the module
    let mut rs_files: Vec<_> = if full_path.is_dir() {
        FileWalker::new(&full_path)
            .for_language("rust")
            .max_depth(2)
            .walk_paths()
            .filter_map(|p| p.ok())
            .collect()
    } else if full_path.extension().is_some_and(|e| e == "rs") && full_path.exists() {
        vec![full_path.clone()]
    } else {
        vec![]
    };

    // Sort for consistent ordering (mod.rs first, then alphabetically)
    rs_files.sort_by(|a, b| {
        let a_is_mod = a.file_name().is_some_and(|n| n == "mod.rs");
        let b_is_mod = b.file_name().is_some_and(|n| n == "mod.rs");
        match (a_is_mod, b_is_mod) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.cmp(b),
        }
    });

    for file_path in rs_files {
        if total_bytes >= max_bytes {
            code.push_str("\n// ... truncated (max size reached) ...\n");
            break;
        }

        if let Ok(content) = std::fs::read_to_string(&file_path) {
            // Get relative path for header
            let relative = file_path
                .strip_prefix(project_path)
                .map(crate::utils::path_to_string)
                .unwrap_or_else(|_| crate::utils::path_to_string(&file_path));

            let header = format!("\n// ═══ {} ═══\n", relative);
            let available = max_bytes.saturating_sub(total_bytes);

            if header.len() + content.len() <= available {
                code.push_str(&header);
                code.push_str(&content);
                total_bytes += header.len() + content.len();
            } else if available > header.len() + 100 {
                // Partial content
                code.push_str(&header);
                let take = available - header.len() - 30;
                code.push_str(truncate_at_boundary(&content, take));
                code.push_str("\n// ... truncated ...\n");
                total_bytes = max_bytes;
            }
        }
    }

    code
}

/// Build prompt for summarizing multiple modules
pub fn build_summary_prompt(modules: &[ModuleSummaryContext]) -> String {
    let mut prompt = String::from(
        "Summarize each module's purpose in 1-2 sentences. Be specific about what it does and how it fits into the system.\n\n\
         Respond in this exact format (one line per module):\n\
         module_id: summary\n\n\
         Modules to summarize:\n\n",
    );

    for module in modules {
        prompt.push_str(&format!("--- {} ---\n", module.module_id));
        prompt.push_str(&format!("Name: {}\n", module.name));
        prompt.push_str(&format!("Lines: {}\n", module.line_count));

        if !module.exports.is_empty() {
            let exports_preview: Vec<_> = module.exports.iter().take(10).collect();
            prompt.push_str(&format!(
                "Exports: {}\n",
                exports_preview
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if !module.code_preview.is_empty() {
            prompt.push_str("Code preview:\n```rust\n");
            prompt.push_str(&module.code_preview);
            prompt.push_str("\n```\n");
        }

        prompt.push('\n');
    }

    prompt
}

/// Parse LLM response into module summaries
pub fn parse_summary_response(response: &str) -> HashMap<String, String> {
    let mut summaries = HashMap::new();

    for line in response.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }

        // Parse "module_id: summary" format
        if let Some((module_id, summary)) = line.split_once(':') {
            let module_id = module_id.trim().to_string();
            let summary = summary.trim().to_string();

            if !module_id.is_empty() && !summary.is_empty() {
                summaries.insert(module_id, summary);
            }
        }
    }

    summaries
}

/// Update module purposes in database (connection-based)
pub fn update_module_purposes(
    conn: &Connection,
    project_id: i64,
    summaries: &HashMap<String, String>,
) -> Result<usize> {
    Ok(update_module_purposes_sync(conn, project_id, summaries)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // parse_summary_response
    // ========================================================================

    #[test]
    fn test_parse_summary_response_single_line() {
        let response = "db/pool: Manages database connection pooling and lifecycle.";
        let summaries = parse_summary_response(response);

        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries.get("db/pool").unwrap(),
            "Manages database connection pooling and lifecycle."
        );
    }

    #[test]
    fn test_parse_summary_response_multiple_lines() {
        let response = "\
db/pool: Manages database connection pooling.
cartographer: Maps codebase structure into modules.
search: Provides semantic and keyword code search.";

        let summaries = parse_summary_response(response);
        assert_eq!(summaries.len(), 3);
        assert!(summaries.contains_key("db/pool"));
        assert!(summaries.contains_key("cartographer"));
        assert!(summaries.contains_key("search"));
    }

    #[test]
    fn test_parse_summary_response_skips_empty_lines() {
        let response = "\
db/pool: Connection pooling.

search: Code search.

";
        let summaries = parse_summary_response(response);
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn test_parse_summary_response_skips_comments_and_headers() {
        let response = "\
# Module Summaries
- Here are the summaries:
db/pool: Connection pooling.";

        let summaries = parse_summary_response(response);
        assert_eq!(summaries.len(), 1);
        assert!(summaries.contains_key("db/pool"));
    }

    #[test]
    fn test_parse_summary_response_skips_empty_module_id_or_summary() {
        let response = "\
: Empty module id
empty_summary:
valid/module: This is valid.";

        let summaries = parse_summary_response(response);
        assert_eq!(summaries.len(), 1);
        assert!(summaries.contains_key("valid/module"));
    }

    #[test]
    fn test_parse_summary_response_empty_input() {
        let summaries = parse_summary_response("");
        assert!(summaries.is_empty());
    }

    #[test]
    fn test_parse_summary_response_colon_in_summary() {
        let response = "db/pool: Manages pools: connection and statement caches.";
        let summaries = parse_summary_response(response);

        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries.get("db/pool").unwrap(),
            "Manages pools: connection and statement caches."
        );
    }

    // ========================================================================
    // build_summary_prompt
    // ========================================================================

    #[test]
    fn test_build_summary_prompt_single_module() {
        let modules = vec![ModuleSummaryContext {
            module_id: "db".to_string(),
            name: "database".to_string(),
            path: "src/db".to_string(),
            exports: vec!["DatabasePool".to_string(), "open".to_string()],
            code_preview: "pub struct DatabasePool { ... }".to_string(),
            line_count: 500,
        }];

        let prompt = build_summary_prompt(&modules);
        assert!(prompt.contains("--- db ---"));
        assert!(prompt.contains("Name: database"));
        assert!(prompt.contains("Lines: 500"));
        assert!(prompt.contains("DatabasePool, open"));
        assert!(prompt.contains("pub struct DatabasePool"));
    }

    #[test]
    fn test_build_summary_prompt_no_exports_no_preview() {
        let modules = vec![ModuleSummaryContext {
            module_id: "utils".to_string(),
            name: "utilities".to_string(),
            path: "src/utils".to_string(),
            exports: vec![],
            code_preview: String::new(),
            line_count: 50,
        }];

        let prompt = build_summary_prompt(&modules);
        assert!(prompt.contains("--- utils ---"));
        assert!(prompt.contains("Name: utilities"));
        assert!(prompt.contains("Lines: 50"));
        // Should not contain "Exports:" or "Code preview:" sections
        assert!(!prompt.contains("Exports:"));
        assert!(!prompt.contains("```rust"));
    }

    #[test]
    fn test_build_summary_prompt_multiple_modules() {
        let modules = vec![
            ModuleSummaryContext {
                module_id: "a".to_string(),
                name: "mod_a".to_string(),
                path: "src/a".to_string(),
                exports: vec!["FnA".to_string()],
                code_preview: String::new(),
                line_count: 100,
            },
            ModuleSummaryContext {
                module_id: "b".to_string(),
                name: "mod_b".to_string(),
                path: "src/b".to_string(),
                exports: vec!["FnB".to_string()],
                code_preview: String::new(),
                line_count: 200,
            },
        ];

        let prompt = build_summary_prompt(&modules);
        assert!(prompt.contains("--- a ---"));
        assert!(prompt.contains("--- b ---"));
    }

    #[test]
    fn test_build_summary_prompt_exports_truncated_to_ten() {
        let exports: Vec<String> = (0..15).map(|i| format!("export_{}", i)).collect();
        let modules = vec![ModuleSummaryContext {
            module_id: "big".to_string(),
            name: "big_module".to_string(),
            path: "src/big".to_string(),
            exports,
            code_preview: String::new(),
            line_count: 1000,
        }];

        let prompt = build_summary_prompt(&modules);
        // Should show only first 10
        assert!(prompt.contains("export_9"));
        assert!(!prompt.contains("export_10"));
    }
}
