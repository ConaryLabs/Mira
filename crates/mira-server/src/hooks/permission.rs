// src/hooks/permission.rs
// Permission hook for Claude Code auto-approval

use anyhow::Result;
use crate::db::Database;
use crate::hooks::{read_hook_input, write_hook_output};
use std::path::PathBuf;

/// Run permission hook
pub async fn run() -> Result<()> {
    let input = read_hook_input()?;

    // Extract tool info from hook input
    let tool_name = input["tool_name"].as_str().unwrap_or("");
    let tool_input = &input["tool_input"];

    // Open database
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let db_path = home.join(".mira/mira.db");
    let db = Database::open(&db_path)?;

    // Check for matching permission rule
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT pattern, match_type FROM permission_rules WHERE tool_name = ?"
    )?;

    let rules: Vec<(String, String)> = stmt
        .query_map([tool_name], |row| Ok((row.get(0)?, row.get(1)?)))
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

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
