//! Permission hook for Claude Code
//!
//! Reads PermissionRequest from stdin, checks against saved rules in the database,
//! and outputs an allow decision if a matching rule is found.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;
use std::io::{self, Read};

#[derive(Debug, Deserialize)]
struct HookInput {
    hook_event_name: String,
    tool_name: Option<String>,
    tool_input: Option<serde_json::Value>,
    cwd: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct PermissionRule {
    id: String,
    tool_name: String,
    input_field: Option<String>,
    input_pattern: Option<String>,
    match_type: String,
    scope: String,
    project_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct HookOutput {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: HookSpecificOutput,
}

#[derive(Debug, Serialize)]
struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    hook_event_name: String,
    decision: Decision,
}

#[derive(Debug, Serialize)]
struct Decision {
    behavior: String,
}

pub async fn run() -> Result<()> {
    // Read stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse hook input
    let hook_input: HookInput = match serde_json::from_str(&input) {
        Ok(h) => h,
        Err(_) => return Ok(()), // Invalid JSON, exit silently
    };

    // Only process PermissionRequest events
    if hook_input.hook_event_name != "PermissionRequest" {
        return Ok(());
    }

    let tool_name = match hook_input.tool_name {
        Some(t) => t,
        None => return Ok(()),
    };

    let tool_input = hook_input.tool_input.unwrap_or(serde_json::json!({}));
    let cwd = hook_input.cwd;

    // Determine input field and value based on tool type
    let (input_field, input_value) = match tool_name.as_str() {
        "Bash" => ("command", tool_input.get("command").and_then(|v| v.as_str())),
        "Edit" | "Write" | "Read" => ("file_path", tool_input.get("file_path").and_then(|v| v.as_str())),
        "Glob" | "Grep" => ("pattern", tool_input.get("pattern").and_then(|v| v.as_str())),
        _ => (tool_name.as_str(), None),
    };

    // Connect to database
    let db_path = dirs::home_dir()
        .map(|h| h.join("Mira/data/mira.db"))
        .expect("Could not find home directory");

    if !db_path.exists() {
        return Ok(()); // No database, exit silently
    }

    let database_url = format!("sqlite://{}", db_path.display());
    let pool = match SqlitePool::connect(&database_url).await {
        Ok(p) => p,
        Err(_) => return Ok(()), // Can't connect, exit silently
    };

    // Get project_id if we have a cwd
    let project_id: Option<i64> = if let Some(ref path) = cwd {
        sqlx::query_scalar("SELECT id FROM projects WHERE path = ?")
            .bind(path)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    // Query for matching rules (using runtime query to avoid sqlx offline cache)
    let rules: Vec<PermissionRule> = sqlx::query_as::<_, PermissionRule>(
        r#"
        SELECT
            id,
            tool_name,
            input_field,
            input_pattern,
            match_type,
            scope,
            project_id
        FROM permission_rules
        WHERE tool_name = ?
          AND (
            (scope = 'global' AND project_id IS NULL) OR
            (scope = 'project' AND project_id = ?)
          )
        ORDER BY
            CASE WHEN project_id IS NOT NULL THEN 0 ELSE 1 END,
            CASE match_type
                WHEN 'exact' THEN 0
                WHEN 'prefix' THEN 1
                WHEN 'glob' THEN 2
                ELSE 3
            END
        "#,
    )
    .bind(&tool_name)
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    // Check each rule for a match
    for rule in rules {
        // If rule has no pattern, it matches all operations for this tool
        let pattern = match &rule.input_pattern {
            Some(p) if !p.is_empty() => p,
            _ => {
                // No pattern means match all for this tool
                update_usage(&pool, &rule.id).await;
                output_allow();
                return Ok(());
            }
        };

        // Check if input field matches and we have a value
        let rule_field = rule.input_field.as_deref().unwrap_or("");
        if rule_field != input_field {
            continue;
        }

        let value = match input_value {
            Some(v) => v,
            None => continue,
        };

        // Check match type
        let matches = match rule.match_type.as_str() {
            "exact" => value == pattern,
            "prefix" => value.starts_with(pattern),
            "glob" => glob_match(value, pattern),
            _ => false,
        };

        if matches {
            update_usage(&pool, &rule.id).await;
            output_allow();
            return Ok(());
        }
    }

    // No matching rule - exit silently to let Claude Code prompt the user
    Ok(())
}

fn glob_match(value: &str, pattern: &str) -> bool {
    // Simple glob matching - convert glob to regex
    let regex_pattern = pattern
        .replace('.', r"\.")
        .replace('*', ".*")
        .replace('?', ".");

    regex::Regex::new(&format!("^{}$", regex_pattern))
        .map(|re| re.is_match(value))
        .unwrap_or(false)
}

async fn update_usage(pool: &SqlitePool, rule_id: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let _ = sqlx::query(
        "UPDATE permission_rules SET times_used = times_used + 1, last_used_at = ? WHERE id = ?"
    )
    .bind(now)
    .bind(rule_id)
    .execute(pool)
    .await;
}

fn output_allow() {
    let output = HookOutput {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PermissionRequest".to_string(),
            decision: Decision {
                behavior: "allow".to_string(),
            },
        },
    };
    println!("{}", serde_json::to_string(&output).unwrap());
}
