// crates/mira-server/src/hooks/post_tool_failure.rs
// Hook handler for PostToolUseFailure events - tracks and learns from tool failures

use crate::db::pool::DatabasePool;
use crate::hooks::{get_db_path, read_hook_input, resolve_project_id, write_hook_output, HookTimer};
use crate::proactive::behavior::BehaviorTracker;
use crate::proactive::EventType;
use anyhow::{Context, Result};
use std::sync::Arc;

/// PostToolUseFailure hook input from Claude Code
#[derive(Debug)]
struct PostToolFailureInput {
    session_id: String,
    tool_name: String,
    error: String,
    is_interrupt: bool,
}

impl PostToolFailureInput {
    fn from_json(json: &serde_json::Value) -> Self {
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
            error: json
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            is_interrupt: json
                .get("is_interrupt")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }
    }
}

/// Run PostToolUseFailure hook
///
/// This hook fires when a tool call fails. We:
/// 1. Log the failure to session_behavior_log
/// 2. Count repeated failures for the same tool in this session
/// 3. If 3+ failures, recall relevant memories and inject as context
pub async fn run() -> Result<()> {
    let _timer = HookTimer::start("PostToolUseFailure");
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
    let failure_input = PostToolFailureInput::from_json(&input);

    eprintln!(
        "[mira] PostToolUseFailure hook triggered (tool: {}, interrupt: {})",
        failure_input.tool_name, failure_input.is_interrupt,
    );

    // Skip if user cancelled (not a real failure)
    if failure_input.is_interrupt {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

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

    // Log the failure event
    let error_summary = crate::utils::truncate(&failure_input.error, 300);
    {
        let session_id = failure_input.session_id.clone();
        let tool_name = failure_input.tool_name.clone();
        let error_summary_clone = error_summary.clone();
        pool.try_interact("tool failure logging", move |conn| {
            let mut tracker = BehaviorTracker::for_session(conn, session_id, project_id);
            let data = serde_json::json!({
                "tool_name": tool_name,
                "error": error_summary_clone,
                "behavior_type": "tool_failure",
            });
            if let Err(e) = tracker.log_event(conn, EventType::ToolUse, data) {
                tracing::debug!("Failed to log tool failure: {e}");
            }
            Ok(())
        })
        .await;
    }

    // Count how many times this tool has failed in the current session
    let tool_name_for_count = failure_input.tool_name.clone();
    let session_id_for_count = failure_input.session_id.clone();
    let failure_count: i64 = pool
        .interact(move |conn| {
            let sql = r#"
                SELECT COUNT(*)
                FROM session_behavior_log
                WHERE session_id = ?
                  AND event_type = 'tool_use'
                  AND json_extract(event_data, '$.behavior_type') = 'tool_failure'
                  AND json_extract(event_data, '$.tool_name') = ?
            "#;
            let count = conn
                .query_row(sql, rusqlite::params![session_id_for_count, tool_name_for_count], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap_or(0);
            Ok::<_, anyhow::Error>(count)
        })
        .await
        .unwrap_or(0);

    eprintln!(
        "[mira] Tool '{}' has failed {} time(s) in this session",
        failure_input.tool_name, failure_count,
    );

    // If 3+ failures, try to recall relevant memories
    if failure_count >= 3 {
        let memories =
            crate::hooks::recall::recall_memories(&pool, project_id, &error_summary).await;

        if !memories.is_empty() {
            let context = format!(
                "[Mira/failure] Tool '{}' has failed {} times. Relevant memories:\n{}",
                failure_input.tool_name,
                failure_count,
                memories.join("\n"),
            );
            let output = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUseFailure",
                    "additionalContext": context,
                }
            });
            write_hook_output(&output);
            return Ok(());
        }
    }

    write_hook_output(&serde_json::json!({}));
    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_input_parses_all_fields() {
        let input = PostToolFailureInput::from_json(&serde_json::json!({
            "session_id": "sess-abc",
            "tool_name": "Read",
            "error": "File not found",
            "is_interrupt": false
        }));
        assert_eq!(input.session_id, "sess-abc");
        assert_eq!(input.tool_name, "Read");
        assert_eq!(input.error, "File not found");
        assert!(!input.is_interrupt);
    }

    #[test]
    fn failure_input_defaults_on_empty_json() {
        let input = PostToolFailureInput::from_json(&serde_json::json!({}));
        assert!(input.session_id.is_empty());
        assert!(input.tool_name.is_empty());
        assert!(input.error.is_empty());
        assert!(!input.is_interrupt);
    }

    #[test]
    fn failure_input_handles_interrupt() {
        let input = PostToolFailureInput::from_json(&serde_json::json!({
            "tool_name": "Bash",
            "is_interrupt": true
        }));
        assert!(input.is_interrupt);
    }

    #[test]
    fn failure_input_ignores_wrong_types() {
        let input = PostToolFailureInput::from_json(&serde_json::json!({
            "session_id": 42,
            "tool_name": true,
            "error": 123,
            "is_interrupt": "yes"
        }));
        assert!(input.session_id.is_empty());
        assert!(input.tool_name.is_empty());
        assert!(input.error.is_empty());
        assert!(!input.is_interrupt);
    }
}
