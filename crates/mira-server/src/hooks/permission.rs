// crates/mira-server/src/hooks/permission.rs
// Permission hook for Claude Code auto-approval

use crate::db::{get_permission_rules_sync, pool::DatabasePool};
use crate::hooks::{read_hook_input, write_hook_output};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;

/// Run permission hook
pub async fn run() -> Result<()> {
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;

    // Extract tool info from hook input
    let tool_name = input["tool_name"].as_str().unwrap_or("").to_string();
    let tool_input = &input["tool_input"];

    // Open database pool
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let db_path = home.join(".mira/mira.db");
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    // Check for matching permission rules
    let rules = pool
        .interact(move |conn| Ok::<_, anyhow::Error>(get_permission_rules_sync(conn, &tool_name)))
        .await?;

    // Collect all matchable strings from tool_input:
    // - The canonical (sorted-key) JSON serialization
    // - Each individual field value (for field-level matching)
    let match_candidates = build_match_candidates(tool_input);

    // Check if any rule matches any candidate
    for (pattern, match_type) in rules {
        let matches = match_candidates
            .iter()
            .any(|candidate| matches_rule(&pattern, &match_type, candidate));

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

/// Build a list of strings to match permission rules against.
///
/// Includes:
/// 1. Canonical JSON with sorted keys (stable across serializations)
/// 2. Each individual string field value (for field-level matching)
/// 3. Each individual non-string scalar as its JSON representation
fn build_match_candidates(tool_input: &serde_json::Value) -> Vec<String> {
    let mut candidates = Vec::new();

    // Add canonical (sorted-key) serialization for whole-object matching
    candidates.push(canonical_json(tool_input));

    // Add individual field values for field-level matching
    if let Some(obj) = tool_input.as_object() {
        for value in obj.values() {
            match value {
                serde_json::Value::String(s) => candidates.push(s.clone()),
                serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
                    candidates.push(value.to_string());
                }
                _ => {}
            }
        }
    }

    candidates
}

/// Produce a canonical JSON string with keys sorted alphabetically.
/// This ensures stable serialization regardless of insertion order.
fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            let entries: Vec<String> = sorted
                .into_iter()
                .map(|(k, v)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap_or_default(),
                        canonical_json(v)
                    )
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(canonical_json).collect();
            format!("[{}]", items.join(","))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Check if a single candidate string matches a rule pattern.
fn matches_rule(pattern: &str, match_type: &str, candidate: &str) -> bool {
    match match_type {
        "exact" => candidate == pattern,
        "prefix" => candidate.starts_with(pattern),
        "glob" => glob_match(pattern, candidate),
        _ => false,
    }
}

/// Simple glob matching (supports * suffix and exact match).
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
    use serde_json::json;

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

    // ============================================================================
    // canonical_json tests
    // ============================================================================

    #[test]
    fn test_canonical_json_sorts_keys() {
        let val = json!({"z": 1, "a": 2, "m": 3});
        let canonical = canonical_json(&val);
        assert_eq!(canonical, r#"{"a":2,"m":3,"z":1}"#);
    }

    #[test]
    fn test_canonical_json_nested_objects() {
        let val = json!({"b": {"z": 1, "a": 2}, "a": 3});
        let canonical = canonical_json(&val);
        assert_eq!(canonical, r#"{"a":3,"b":{"a":2,"z":1}}"#);
    }

    #[test]
    fn test_canonical_json_stable_across_orderings() {
        // Simulate two JSON objects with same content but different insertion order
        let mut map1 = serde_json::Map::new();
        map1.insert("action".to_string(), json!("recall"));
        map1.insert("query".to_string(), json!("auth"));
        let val1 = serde_json::Value::Object(map1);

        let mut map2 = serde_json::Map::new();
        map2.insert("query".to_string(), json!("auth"));
        map2.insert("action".to_string(), json!("recall"));
        let val2 = serde_json::Value::Object(map2);

        assert_eq!(canonical_json(&val1), canonical_json(&val2));
    }

    // ============================================================================
    // build_match_candidates tests
    // ============================================================================

    #[test]
    fn test_build_match_candidates_includes_field_values() {
        let input = json!({"action": "recall", "query": "auth patterns"});
        let candidates = build_match_candidates(&input);

        // Should include canonical JSON
        assert!(
            candidates
                .iter()
                .any(|c| c.contains("action") && c.contains("recall"))
        );
        // Should include individual field values
        assert!(candidates.contains(&"recall".to_string()));
        assert!(candidates.contains(&"auth patterns".to_string()));
    }

    #[test]
    fn test_build_match_candidates_non_object() {
        let input = json!("just a string");
        let candidates = build_match_candidates(&input);
        // Should have canonical JSON only (no field extraction for non-objects)
        assert_eq!(candidates.len(), 1);
    }

    // ============================================================================
    // matches_rule tests
    // ============================================================================

    #[test]
    fn test_matches_rule_field_level_glob() {
        // A glob pattern matching a field value (not the full serialized JSON)
        let input = json!({"action": "recall", "query": "/home/peter/project"});
        let candidates = build_match_candidates(&input);

        // Should match the "query" field value via glob
        assert!(
            candidates
                .iter()
                .any(|c| matches_rule("/home/*", "glob", c))
        );
    }

    #[test]
    fn test_matches_rule_exact_on_field() {
        let input = json!({"action": "remember", "content": "test"});
        let candidates = build_match_candidates(&input);

        assert!(
            candidates
                .iter()
                .any(|c| matches_rule("remember", "exact", c))
        );
        assert!(
            !candidates
                .iter()
                .any(|c| matches_rule("recall", "exact", c))
        );
    }
}
