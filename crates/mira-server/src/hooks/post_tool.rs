// crates/mira-server/src/hooks/post_tool.rs
// PostToolUse hook handler - tracks file changes and provides hints

use crate::db::pool::DatabasePool;
use crate::hooks::{
    HookTimer, get_db_path, read_hook_input, resolve_project_id, write_hook_output,
};
use crate::proactive::behavior::BehaviorTracker;
use anyhow::Result;
use std::sync::Arc;

/// PostToolUse hook input from Claude Code
#[derive(Debug)]
struct PostToolInput {
    session_id: String,
    tool_name: String,
    file_path: Option<String>,
}

impl PostToolInput {
    fn from_json(json: &serde_json::Value) -> Self {
        let file_path = json
            .get("tool_input")
            .and_then(|ti| ti.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Self {
            session_id: json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            tool_name: json
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            file_path,
        }
    }
}

/// Run PostToolUse hook
///
/// This hook fires after Write/Edit tools complete. We:
/// 1. Queue the file for re-indexing
/// 2. Check for related tests that might need updating
/// 3. Provide hints about the changed file
pub async fn run() -> Result<()> {
    let _timer = HookTimer::start("PostToolUse");
    let input = read_hook_input()?;
    let post_input = PostToolInput::from_json(&input);

    eprintln!(
        "[mira] PostToolUse hook triggered (tool: {}, file: {:?})",
        post_input.tool_name,
        post_input.file_path.as_deref().unwrap_or("none")
    );

    // Only process Write/Edit operations with file paths
    let Some(file_path) = post_input.file_path else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Open database
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    // Get current project
    let Some(project_id) = resolve_project_id(&pool).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    let mut context_parts: Vec<String> = Vec::new();

    // Log behavior events for proactive intelligence
    {
        let session_id = post_input.session_id.clone();
        let tool_name = post_input.tool_name.clone();
        let file_path_clone = file_path.clone();
        pool.try_interact("behavior tracking", move |conn| {
            let mut tracker = BehaviorTracker::for_session(conn, session_id, project_id);

            // Log tool use
            if let Err(e) = tracker.log_tool_use(conn, &tool_name, None) {
                tracing::debug!("Failed to log tool use: {e}");
            }

            // Log file access
            if let Err(e) = tracker.log_file_access(conn, &file_path_clone, &tool_name) {
                tracing::debug!("Failed to log file access: {e}");
            }

            Ok(())
        })
        .await;
    }

    // Queue file for re-indexing (background)
    {
        let file_path_clone = file_path.clone();
        pool.try_interact("queue file indexing", move |conn| {
            queue_file_for_indexing(conn, project_id, &file_path_clone)
        })
        .await;
    }

    // Track file ownership for team intelligence (only for file-mutating tools)
    let is_write_tool = matches!(
        post_input.tool_name.as_str(),
        "Write" | "Edit" | "NotebookEdit" | "MultiEdit"
    );
    if is_write_tool
        && let Some(membership) =
            crate::hooks::session::read_team_membership_from_db(&pool, &post_input.session_id).await
    {
        let sid = post_input.session_id.clone();
        let member = membership.member_name.clone();
        let fp = file_path.clone();
        let tool = post_input.tool_name.clone();
        let team_id = membership.team_id;
        if let Err(e) = pool
            .run(move |conn| {
                crate::db::record_file_ownership_sync(conn, team_id, &sid, &member, &fp, &tool)
            })
            .await
        {
            eprintln!("[mira] File ownership tracking failed: {}", e);
        }

        // Check for conflicts with other teammates
        let pool_clone = pool.clone();
        let sid = post_input.session_id.clone();
        let tid = membership.team_id;
        let conflicts: Vec<crate::db::FileConflict> = pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(crate::db::get_file_conflicts_sync(conn, tid, &sid))
            })
            .await
            .unwrap_or_default();

        if !conflicts.is_empty() {
            let warnings: Vec<String> = conflicts
                .iter()
                .take(3)
                .map(|c| {
                    format!(
                        "⚠ {} also edited {} ({})",
                        c.other_member_name, c.file_path, c.operation
                    )
                })
                .collect();
            context_parts.push(format!(
                "[Team] File conflict warning:\n{}",
                warnings.join("\n")
            ));
            eprintln!(
                "[mira] {} file conflict(s) detected with teammates",
                conflicts.len()
            );
        }
    }

    // Check for related test files
    let test_hint = find_related_tests(&file_path);
    if let Some(hint) = test_hint {
        context_parts.push(hint);
    }

    // Check if this is a significant file type that might need attention
    let file_hint = get_file_type_hint(&file_path);
    if let Some(hint) = file_hint {
        context_parts.push(hint);
    }

    // Build output
    let output = if context_parts.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PostToolUse",
                "additionalContext": context_parts.join("\n\n")
            }
        })
    };

    write_hook_output(&output);
    Ok(())
}

/// Queue a file for background re-indexing
fn queue_file_for_indexing(
    conn: &rusqlite::Connection,
    project_id: i64,
    file_path: &str,
) -> Result<()> {
    // Add to pending_files table if it exists
    let sql = r#"
        INSERT OR REPLACE INTO pending_files (project_id, path, queued_at, status)
        VALUES (?, ?, datetime('now'), 'pending')
    "#;

    match conn.execute(sql, rusqlite::params![project_id, file_path]) {
        Ok(_) => {
            eprintln!("[mira] Queued {} for re-indexing", file_path);
        }
        Err(e) => {
            // Table might not exist - that's ok
            eprintln!("[mira] Could not queue file: {}", e);
        }
    }

    Ok(())
}

/// Find related test files for a source file
fn find_related_tests(file_path: &str) -> Option<String> {
    let path = std::path::Path::new(file_path);
    let file_name = path.file_stem()?.to_str()?;
    let extension = path.extension()?.to_str()?;

    // Skip if already a test file
    if file_name.contains("test") || file_name.contains("spec") {
        return None;
    }

    // Common test file patterns
    let test_patterns = match extension {
        "rs" => vec![
            format!("tests/{}.rs", file_name),
            format!("tests/{}_test.rs", file_name),
            format!("{}_test.rs", file_name),
        ],
        "ts" | "tsx" | "js" | "jsx" => vec![
            format!("{}.test.{}", file_name, extension),
            format!("{}.spec.{}", file_name, extension),
            format!("__tests__/{}.{}", file_name, extension),
        ],
        "py" => vec![
            format!("test_{}.py", file_name),
            format!("{}_test.py", file_name),
            format!("tests/test_{}.py", file_name),
        ],
        "go" => vec![format!("{}_test.go", file_name)],
        _ => return None,
    };

    // Check if any test files exist
    let parent = path.parent()?;
    for pattern in &test_patterns {
        let test_path = parent.join(pattern);
        if test_path.exists() {
            return Some(format!(
                "Related test file exists: {}. Consider updating tests if behavior changed.",
                test_path.display()
            ));
        }
    }

    // Suggest creating tests for significant files
    if is_significant_file(file_path) {
        Some(format!(
            "No test file found for {}. Consider adding tests in one of: {}",
            file_name,
            test_patterns.first().unwrap_or(&"tests/".to_string())
        ))
    } else {
        None
    }
}

/// Check if a file is significant (likely needs tests)
fn is_significant_file(file_path: &str) -> bool {
    let path = file_path.to_lowercase();

    // Skip config, docs, etc.
    if path.contains("config")
        || path.contains(".md")
        || path.contains(".json")
        || path.contains(".toml")
        || path.contains(".yaml")
        || path.contains(".yml")
        || path.contains("readme")
        || path.contains("license")
    {
        return false;
    }

    // Source files are significant
    path.ends_with(".rs")
        || path.ends_with(".ts")
        || path.ends_with(".tsx")
        || path.ends_with(".js")
        || path.ends_with(".jsx")
        || path.ends_with(".py")
        || path.ends_with(".go")
}

/// Get hints based on file type
fn get_file_type_hint(file_path: &str) -> Option<String> {
    let path = file_path.to_lowercase();

    // Security-sensitive files
    if path.contains("auth") || path.contains("login") || path.contains("password") {
        return Some("This file may contain security-sensitive code. Consider security implications of changes.".to_string());
    }

    // Database/migration files
    if path.contains("migration") || path.contains("schema") {
        return Some(
            "Database schema change detected. Ensure migrations are reversible and tested."
                .to_string(),
        );
    }

    // Config files
    if path.ends_with(".env") || path.contains("config") {
        return Some(
            "Configuration file changed. Verify environment-specific settings.".to_string(),
        );
    }

    None
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── PostToolInput::from_json ────────────────────────────────────────────

    #[test]
    fn post_input_parses_all_fields() {
        let input = PostToolInput::from_json(&serde_json::json!({
            "session_id": "sess-abc",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": "/src/main.rs"
            }
        }));
        assert_eq!(input.session_id, "sess-abc");
        assert_eq!(input.tool_name, "Edit");
        assert_eq!(input.file_path.as_deref(), Some("/src/main.rs"));
    }

    #[test]
    fn post_input_defaults_on_empty_json() {
        let input = PostToolInput::from_json(&serde_json::json!({}));
        assert!(input.session_id.is_empty());
        assert!(input.tool_name.is_empty());
        assert!(input.file_path.is_none());
    }

    #[test]
    fn post_input_missing_file_path() {
        let input = PostToolInput::from_json(&serde_json::json!({
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {
                "command": "ls"
            }
        }));
        assert_eq!(input.tool_name, "Bash");
        assert!(input.file_path.is_none());
    }

    #[test]
    fn post_input_ignores_wrong_types() {
        let input = PostToolInput::from_json(&serde_json::json!({
            "session_id": 42,
            "tool_name": true,
            "tool_input": "not-an-object"
        }));
        assert!(input.session_id.is_empty());
        assert!(input.tool_name.is_empty());
        assert!(input.file_path.is_none());
    }

    // ── is_significant_file ────────────────────────────────────────────────

    #[test]
    fn significant_source_files() {
        assert!(is_significant_file("src/main.rs"));
        assert!(is_significant_file("app/page.tsx"));
        assert!(is_significant_file("lib/utils.ts"));
        assert!(is_significant_file("handler.js"));
        assert!(is_significant_file("views.py"));
        assert!(is_significant_file("server.go"));
        assert!(is_significant_file("/home/user/project/src/lib.jsx"));
    }

    #[test]
    fn insignificant_config_and_docs() {
        assert!(!is_significant_file("README.md"));
        assert!(!is_significant_file("Cargo.toml"));
        assert!(!is_significant_file("package.json"));
        assert!(!is_significant_file("config.yaml"));
        assert!(!is_significant_file("settings.yml"));
        assert!(!is_significant_file("LICENSE"));
    }

    #[test]
    fn insignificant_mixed_case() {
        // config in path should disqualify even .rs files
        assert!(!is_significant_file("src/config.rs"));
    }

    #[test]
    fn insignificant_unknown_extension() {
        assert!(!is_significant_file("data.csv"));
        assert!(!is_significant_file("image.png"));
    }

    // ── get_file_type_hint ─────────────────────────────────────────────────

    #[test]
    fn hint_auth_files() {
        let hint = get_file_type_hint("src/auth/handler.rs");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("security"));
    }

    #[test]
    fn hint_login_files() {
        let hint = get_file_type_hint("components/LoginForm.tsx");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("security"));
    }

    #[test]
    fn hint_password_files() {
        let hint = get_file_type_hint("utils/password_hash.py");
        assert!(hint.is_some());
    }

    #[test]
    fn hint_migration_files() {
        let hint = get_file_type_hint("db/migrations/001_create_users.sql");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("migration"));
    }

    #[test]
    fn hint_schema_files() {
        let hint = get_file_type_hint("src/db/schema.rs");
        assert!(hint.is_some());
    }

    #[test]
    fn hint_config_files() {
        let hint = get_file_type_hint("app/config.toml");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("Configuration"));
    }

    #[test]
    fn hint_env_files() {
        let hint = get_file_type_hint("project/.env");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("Configuration"));
    }

    #[test]
    fn hint_normal_file_returns_none() {
        assert!(get_file_type_hint("src/utils/helpers.rs").is_none());
        assert!(get_file_type_hint("lib/math.ts").is_none());
    }

    // ── find_related_tests ─────────────────────────────────────────────────

    #[test]
    fn find_tests_skips_test_files() {
        assert!(find_related_tests("src/foo_test.rs").is_none());
        assert!(find_related_tests("src/foo.spec.ts").is_none());
        assert!(find_related_tests("tests/test_bar.py").is_none());
    }

    #[test]
    fn find_tests_unknown_extension_returns_none() {
        assert!(find_related_tests("data/file.csv").is_none());
    }

    #[test]
    fn find_tests_no_extension_returns_none() {
        assert!(find_related_tests("Makefile").is_none());
    }

    #[test]
    fn find_tests_rs_file_no_tests_exist() {
        // In test env, no test files exist on disk, so we get the
        // "no test file found" suggestion for significant files
        let result = find_related_tests("/nonexistent/src/handler.rs");
        // handler.rs is significant (source code), so we get a suggestion
        assert!(result.is_some());
        assert!(result.unwrap().contains("No test file found"));
    }

    #[test]
    fn find_tests_config_file_no_suggestion() {
        // config files are not significant, so no test suggestion
        let result = find_related_tests("/nonexistent/src/config.rs");
        assert!(result.is_none());
    }

    #[test]
    fn find_tests_js_file_no_tests_exist() {
        let result = find_related_tests("/nonexistent/src/app.js");
        assert!(result.is_some());
        assert!(result.unwrap().contains("No test file found"));
    }

    #[test]
    fn find_tests_py_file_suggestion() {
        let result = find_related_tests("/nonexistent/src/views.py");
        assert!(result.is_some());
        assert!(result.unwrap().contains("test_views.py"));
    }

    #[test]
    fn find_tests_go_file_suggestion() {
        let result = find_related_tests("/nonexistent/src/server.go");
        assert!(result.is_some());
        assert!(result.unwrap().contains("server_test.go"));
    }
}
