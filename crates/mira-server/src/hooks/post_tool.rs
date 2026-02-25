// crates/mira-server/src/hooks/post_tool.rs
// PostToolUse hook handler - tracks file changes and detects team conflicts

use crate::hooks::ast_diff;
use crate::hooks::{HookTimer, read_hook_input, write_hook_output};
use anyhow::{Context, Result};

/// PostToolUse hook input from Claude Code
#[derive(Debug)]
struct PostToolInput {
    session_id: String,
    tool_name: String,
    file_path: Option<String>,
    command: Option<String>,
}

impl PostToolInput {
    fn from_json(json: &serde_json::Value) -> Self {
        let tool_input = json.get("tool_input");
        let file_path = tool_input
            .and_then(|ti| ti.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let command = tool_input
            .and_then(|ti| ti.get("command"))
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
            command,
        }
    }
}

/// Run PostToolUse hook
///
/// This hook fires after any tool that provides a file_path. We:
/// 1. Track file access for the session (behavior logging for all tools)
/// 2. Detect team file conflicts when write tools (Write/Edit) modify shared files
///
/// TODO(injection_feedback): Wire `ContextManager::record_response_feedback()` here
/// to close the injection feedback learning loop. Challenges:
/// - This hook runs as a separate CLI process communicating via IPC, so there is
///   no access to `ContextInjectionManager` or `InjectionAnalytics`.
/// - The hook input does not include response/output text to compare against.
///
/// To implement: add a new IPC command (e.g. `RecordResponseFeedback`) that
/// forwards the call to `InjectionAnalytics::record_response_feedback()` on the
/// server side, and extend the hook input to include `tool_output` or
/// `response_text` from Claude Code's PostToolUse payload.
pub async fn run() -> Result<()> {
    let _timer = HookTimer::start("PostToolUse");
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
    let post_input = PostToolInput::from_json(&input);

    tracing::debug!(
        tool = %post_input.tool_name,
        file = post_input.file_path.as_deref().unwrap_or("none"),
        "PostToolUse hook triggered"
    );

    // Connect via IPC (falls back to direct DB)
    let mut client = crate::ipc::client::HookClient::connect().await;

    // Get current project
    let sid = Some(post_input.session_id.as_str()).filter(|s| !s.is_empty());
    let Some((project_id, _)) = client.resolve_project(None, sid).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Resolve error patterns if this tool had recent failures in this session.
    // Only resolves a pattern when THAT SPECIFIC fingerprint had 3+ failures
    // in this session (not just any failure of the same tool).
    let resolved = client
        .resolve_error_patterns(project_id, &post_input.session_id, &post_input.tool_name)
        .await;
    if resolved {
        tracing::info!(
            tool = %post_input.tool_name,
            "Resolved error pattern for tool"
        );
    }

    // Handle Bash commands: detect file-modifying commands and log them
    if post_input.tool_name == "Bash" {
        if let Some(ref command) = post_input.command
            && is_file_modifying_command(command)
        {
            let cmd = crate::utils::truncate(command, 500);
            let data = serde_json::json!({
                "behavior_type": "bash_file_modify",
                "command": cmd,
            });
            client
                .log_behavior(&post_input.session_id, project_id, "tool_use", data)
                .await;
        }
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    // Only process Write/Edit operations with file paths
    let Some(file_path) = post_input.file_path else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    let mut context_parts: Vec<String> = Vec::new();

    // Log behavior events for proactive intelligence
    {
        let data = serde_json::json!({
            "tool_name": post_input.tool_name,
            "file_path": file_path,
            "behavior_type": "file_access",
        });
        client
            .log_behavior(&post_input.session_id, project_id, "tool_use", data)
            .await;
    }

    // Track file ownership for team intelligence (only for file-mutating tools)
    let is_write_tool = matches!(
        post_input.tool_name.as_str(),
        "Write" | "Edit" | "NotebookEdit" | "MultiEdit"
    );

    if is_write_tool
        && let Some(membership) = client.get_team_membership(&post_input.session_id).await
    {
        client
            .record_file_ownership(
                membership.team_id,
                &post_input.session_id,
                &membership.member_name,
                &file_path,
                &post_input.tool_name,
            )
            .await;

        // Check for conflicts with other teammates
        let conflicts = client
            .get_file_conflicts(membership.team_id, &post_input.session_id)
            .await;

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
            tracing::warn!(
                count = conflicts.len(),
                "File conflict(s) detected with teammates"
            );
        }
    }

    // AST-level change detection for Write/Edit tools
    if is_write_tool
        && let Some((_, project_path)) = client.resolve_project(None, sid).await
        && let Some(old_content) = ast_diff::get_previous_content(&file_path, &project_path).await
        && let Ok(new_content) = tokio::fs::read_to_string(&file_path).await
        && let Some(changes) = ast_diff::detect_structural_changes(
            std::path::Path::new(&file_path),
            &old_content,
            &new_content,
        )
        .filter(|c| !c.is_empty())
    {
        let change_summary: Vec<String> = changes
            .iter()
            .take(5)
            .map(|c| {
                format!(
                    "{}: {} `{}` at line {}",
                    c.change_kind, c.symbol_kind, c.symbol_name, c.line_number
                )
            })
            .collect();

        let data = serde_json::json!({
            "behavior_type": "structural_change",
            "file_path": file_path,
            "changes": change_summary,
        });
        client
            .log_behavior(&post_input.session_id, project_id, "ast_diff", data)
            .await;

        tracing::debug!(
            file = %file_path,
            count = changes.len(),
            "AST diff detected structural changes"
        );
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

/// Check if a Bash command appears to modify files.
///
/// Returns true for commands that create, move, delete, or modify files.
/// Returns false for read-only commands like ls, cat, grep, git status, etc.
fn is_file_modifying_command(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Check for file-modifying patterns first (anywhere in the command,
    // to catch pipelines like "find ... | xargs mv ..." and redirects
    // like "echo ... > file")

    // Redirect operators
    if trimmed.contains("> ") || trimmed.contains(">> ") {
        return true;
    }

    // File-modifying commands (anywhere in pipeline)
    const MODIFY_COMMANDS: &[&str] = &["mv ", "cp ", "rm ", "mkdir ", "touch ", "chmod ", "chown "];
    for cmd in MODIFY_COMMANDS {
        if trimmed.contains(cmd) {
            return true;
        }
    }

    // Git operations that modify working tree
    const GIT_MODIFY_PREFIXES: &[&str] = &["git checkout", "git merge", "git rebase", "git reset"];
    for prefix in GIT_MODIFY_PREFIXES {
        if trimmed.contains(prefix) {
            return true;
        }
    }

    false
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
        assert_eq!(input.command.as_deref(), Some("ls"));
    }

    #[test]
    fn post_input_parses_command() {
        let input = PostToolInput::from_json(&serde_json::json!({
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {
                "command": "mv foo.rs bar.rs"
            }
        }));
        assert_eq!(input.command.as_deref(), Some("mv foo.rs bar.rs"));
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
        assert!(input.command.is_none());
    }

    // ── is_file_modifying_command ───────────────────────────────────────────

    #[test]
    fn detects_mv_command() {
        assert!(is_file_modifying_command("mv old.rs new.rs"));
    }

    #[test]
    fn detects_cp_command() {
        assert!(is_file_modifying_command("cp src/a.rs src/b.rs"));
    }

    #[test]
    fn detects_rm_command() {
        assert!(is_file_modifying_command("rm -rf target/"));
    }

    #[test]
    fn detects_mkdir_command() {
        assert!(is_file_modifying_command("mkdir -p src/hooks"));
    }

    #[test]
    fn detects_touch_command() {
        assert!(is_file_modifying_command("touch new_file.rs"));
    }

    #[test]
    fn detects_chmod_command() {
        assert!(is_file_modifying_command("chmod +x script.sh"));
    }

    #[test]
    fn detects_redirect() {
        assert!(is_file_modifying_command("echo 'hello' > output.txt"));
    }

    #[test]
    fn detects_append_redirect() {
        assert!(is_file_modifying_command("echo 'more' >> output.txt"));
    }

    #[test]
    fn detects_git_checkout() {
        assert!(is_file_modifying_command("git checkout main"));
    }

    #[test]
    fn detects_git_merge() {
        assert!(is_file_modifying_command("git merge feature-branch"));
    }

    #[test]
    fn detects_git_rebase() {
        assert!(is_file_modifying_command("git rebase main"));
    }

    #[test]
    fn detects_git_reset() {
        assert!(is_file_modifying_command("git reset --hard HEAD~1"));
    }

    #[test]
    fn skips_ls() {
        assert!(!is_file_modifying_command("ls -la"));
    }

    #[test]
    fn skips_cat() {
        assert!(!is_file_modifying_command("cat foo.rs"));
    }

    #[test]
    fn skips_grep() {
        assert!(!is_file_modifying_command("grep -r 'pattern' src/"));
    }

    #[test]
    fn skips_git_status() {
        assert!(!is_file_modifying_command("git status"));
    }

    #[test]
    fn skips_git_log() {
        assert!(!is_file_modifying_command("git log --oneline"));
    }

    #[test]
    fn skips_git_diff() {
        assert!(!is_file_modifying_command("git diff HEAD~1"));
    }

    #[test]
    fn skips_cargo_test() {
        assert!(!is_file_modifying_command("cargo test -- --nocapture"));
    }

    #[test]
    fn skips_cargo_check() {
        assert!(!is_file_modifying_command("cargo check"));
    }

    #[test]
    fn skips_echo_without_redirect() {
        assert!(!is_file_modifying_command("echo hello world"));
    }

    #[test]
    fn detects_piped_mv() {
        // "find ... | xargs mv" - contains "mv " inside pipeline
        assert!(is_file_modifying_command(
            "find . -name '*.bak' | xargs mv target/"
        ));
    }

    #[test]
    fn handles_empty_command() {
        assert!(!is_file_modifying_command(""));
    }

    #[test]
    fn handles_whitespace_command() {
        assert!(!is_file_modifying_command("   "));
    }
}
