// crates/mira-server/src/hooks/post_tool.rs
// PostToolUse hook handler - tracks file changes and detects team conflicts

use crate::db::pool::DatabasePool;
use crate::hooks::{
    HookTimer, get_db_path, read_hook_input, resolve_project_id, write_hook_output,
};
use crate::proactive::behavior::BehaviorTracker;
use anyhow::{Context, Result};
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
/// This hook fires after any tool that provides a file_path. We:
/// 1. Track file access for the session (behavior logging for all tools)
/// 2. Detect team file conflicts when write tools (Write/Edit) modify shared files
pub async fn run() -> Result<()> {
    let _timer = HookTimer::start("PostToolUse");
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
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
    let pool = match DatabasePool::open(&db_path).await {
        Ok(p) => Arc::new(p),
        Err(_) => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

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
                        "{} also edited {} ({})",
                        c.other_member_name, c.file_path, c.operation
                    )
                })
                .collect();
            context_parts.push(format!(
                "[Mira/conflict] File conflict warning:\n{}",
                warnings.join("\n")
            ));
            eprintln!(
                "[mira] {} file conflict(s) detected with teammates",
                conflicts.len()
            );
        }
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
}
