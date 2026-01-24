// src/hooks/permission.rs
// Permission hook for Claude Code auto-approval

use anyhow::Result;
use crate::db::{pool::DatabasePool, get_permission_rules_sync};
use crate::hooks::{read_hook_input, write_hook_output};
use std::path::PathBuf;

/// Run permission hook
pub async fn run() -> Result<()> {
    let input = read_hook_input()?;

    // Extract tool info from hook input
    let tool_name = input["tool_name"].as_str().unwrap_or("").to_string();
    let tool_input = &input["tool_input"];

    // Open database pool
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let db_path = home.join(".mira/mira.db");
    let pool = DatabasePool::open(&db_path).await?;

    // Check for matching permission rules
    let rules = pool.interact(move |conn| {
        Ok::<_, anyhow::Error>(get_permission_rules_sync(conn, &tool_name))
    }).await?;

    // Check if any rule matches
    for (pattern, match_type) in rules {
        let input_str = serde_json::to_string(tool_input).unwrap_or_default();

        let matches = match match_type.as_str() {
            "exact" => input_str == pattern,
            "prefix" => input_str.starts_with(&pattern),
            "glob" => glob_match(&pattern, &input_str),
            _ => false,
        };

        if matches {
            // Auto-approve
            write_hook_output(&serde_json::json!({
                "decision": "allow"
            }));
            return Ok(());
        }
    }

    // No matching rule - let Claude Code handle it
    write_hook_output(&serde_json::json!({}));
    Ok(())
}

/// Simple glob matching
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return text.starts_with(prefix);
    }
    pattern == text
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // glob_match tests
    // ============================================================================

    #[test]
    fn test_glob_match_wildcard_only() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "some/path/here"));
    }

    #[test]
    fn test_glob_match_prefix_wildcard() {
        assert!(glob_match("prefix*", "prefix_something"));
        assert!(glob_match("prefix*", "prefix"));
        assert!(glob_match("/home/*", "/home/user"));
        assert!(!glob_match("prefix*", "other"));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exact_more"));
        assert!(!glob_match("exact", "other"));
    }

    #[test]
    fn test_glob_match_empty_pattern() {
        assert!(glob_match("", "")); // Exact match of empty strings
        assert!(!glob_match("", "something"));
    }

    #[test]
    fn test_glob_match_command_patterns() {
        assert!(glob_match("git*", "git status"));
        assert!(glob_match("npm*", "npm install"));
        assert!(glob_match("cargo*", "cargo build --release"));
    }
}
