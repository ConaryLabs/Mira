// crates/mira-server/src/hooks/post_tool.rs
// PostToolUse hook handler - tracks file changes and provides hints

use crate::db::pool::DatabasePool;
use crate::hooks::{HookTimer, get_db_path, read_hook_input, resolve_project_id, write_hook_output};
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
        let pool_clone = pool.clone();
        let session_id = post_input.session_id.clone();
        let tool_name = post_input.tool_name.clone();
        let file_path_clone = file_path.clone();
        let _ = pool_clone
            .interact(move |conn| {
                let mut tracker = BehaviorTracker::for_session(conn, session_id, project_id);

                // Log tool use
                let _ = tracker.log_tool_use(conn, &tool_name, None);

                // Log file access
                let _ = tracker.log_file_access(conn, &file_path_clone, &tool_name);

                Ok::<_, anyhow::Error>(())
            })
            .await;
    }

    // Queue file for re-indexing (background)
    {
        let pool_clone = pool.clone();
        let file_path_clone = file_path.clone();
        let _ = pool_clone
            .interact(move |conn| queue_file_for_indexing(conn, project_id, &file_path_clone))
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
        let pool_clone = pool.clone();
        let sid = post_input.session_id.clone();
        let member = membership.member_name.clone();
        let fp = file_path.clone();
        let tool = post_input.tool_name.clone();
        let team_id = membership.team_id;
        let result = pool_clone
            .interact(move |conn| {
                crate::db::record_file_ownership_sync(conn, team_id, &sid, &member, &fp, &tool)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await;

        if let Err(e) = result {
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
