// crates/mira-server/src/hooks/post_tool_failure.rs
// Hook handler for PostToolUseFailure events - tracks and learns from tool failures

use crate::hooks::{HookTimer, read_hook_input, write_hook_output};
use anyhow::{Context, Result};

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

    tracing::debug!(
        tool = %failure_input.tool_name,
        interrupt = failure_input.is_interrupt,
        "PostToolUseFailure hook triggered"
    );

    // Skip if user cancelled (not a real failure)
    if failure_input.is_interrupt {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    // Connect via IPC (falls back to direct DB)
    let mut client = crate::ipc::client::HookClient::connect().await;

    // Get current project
    let sid = Some(failure_input.session_id.as_str()).filter(|s| !s.is_empty());
    let Some((project_id, _project_path)) = client.resolve_project(None, sid).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Log the failure event (redact sensitive data before truncation)
    let redacted_error = crate::utils::redact_sensitive(&failure_input.error);
    let error_summary = crate::utils::truncate(&redacted_error, 300);

    // Compute fingerprint once — used in both behavior log and error pattern storage
    let (fingerprint, template) =
        crate::db::error_fingerprint(&failure_input.tool_name, &error_summary);

    {
        let data = serde_json::json!({
            "tool_name": failure_input.tool_name,
            "error": error_summary,
            "behavior_type": "tool_failure",
            "error_fingerprint": fingerprint,
        });
        client
            .log_behavior(&failure_input.session_id, project_id, "tool_failure", data)
            .await;
    }

    // Store/update error pattern for cross-session learning
    client
        .store_error_pattern(
            project_id,
            &failure_input.tool_name,
            &fingerprint,
            &template,
            &error_summary,
            &failure_input.session_id,
        )
        .await;

    // Count how many times this tool has failed in the current session
    let failure_count = client
        .count_session_failures(&failure_input.session_id, &failure_input.tool_name)
        .await;

    tracing::info!(
        tool = %failure_input.tool_name,
        failure_count,
        "Tool has failed in this session"
    );

    // If 3+ failures, look up known fixes first, then fall back to memory recall
    if failure_count >= 3 {
        // Check for resolved error patterns (cross-session fix knowledge)
        let fix_context = client
            .lookup_resolved_pattern(project_id, &failure_input.tool_name, &fingerprint)
            .await;

        if let Some(fix_description) = fix_context {
            let context = format!(
                "[Mira/fix] Tool '{}' failed ({}x). A similar error was resolved before:\n  Fix: {}",
                failure_input.tool_name,
                failure_count,
                crate::utils::truncate(&fix_description, 300),
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
