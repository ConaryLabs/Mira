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
pub async fn run() -> Result<()> {
    let _timer = HookTimer::start("PostToolUse");
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
    let post_input = PostToolInput::from_json(&input);

    eprintln!(
        "[mira] PostToolUse hook triggered (tool: {}, file: {:?})",
        post_input.tool_name,
        post_input.file_path.as_deref().unwrap_or("none")
    );

    // Open database (shared across all branches)
    let db_path = get_db_path();
    let pool = match DatabasePool::open_hook(&db_path).await {
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

    // Resolve error patterns if this tool had recent failures in this session.
    // Only resolves a pattern when THAT SPECIFIC fingerprint had 3+ failures
    // in this session (not just any failure of the same tool).
    {
        let session_id = post_input.session_id.clone();
        let tool_name = post_input.tool_name.clone();
        pool.try_interact("error pattern resolution", move |conn| {
            // Get all candidate patterns (global occurrence_count >= 3)
            let candidates = crate::db::get_unresolved_patterns_for_tool_sync(
                conn, project_id, &tool_name, &session_id,
            );

            // For each candidate, get its session failure count and most recent
            // failure timestamp. The fingerprint that failed most recently is the
            // one most likely fixed by this success. Only resolve ONE per success.
            // (fingerprint, count, max_sequence_position)
            let mut best: Option<(String, i64, i64)> = None;
            for (_id, fingerprint) in &candidates {
                let row: Option<(i64, i64)> = conn
                    .query_row(
                        "SELECT COUNT(*), COALESCE(MAX(sequence_position), 0)
                         FROM session_behavior_log
                         WHERE session_id = ? AND project_id = ?
                           AND event_type = 'tool_failure'
                           AND json_extract(event_data, '$.error_fingerprint') = ?",
                        rusqlite::params![&session_id, project_id, fingerprint],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .ok();

                if let Some((count, max_seq)) = row
                    && count >= 3
                {
                    let dominated = match &best {
                        None => true,
                        Some((_, _, best_seq)) => max_seq > *best_seq,
                    };
                    if dominated {
                        best = Some((fingerprint.clone(), count, max_seq));
                    }
                }
            }

            if let Some((fingerprint, session_fp_count, _)) = best {
                let _ = crate::db::resolve_error_pattern_sync(
                    conn,
                    project_id,
                    &tool_name,
                    &fingerprint,
                    &session_id,
                    &format!(
                        "Tool '{}' succeeded after {} session failures of this pattern",
                        tool_name, session_fp_count
                    ),
                );
                eprintln!(
                    "[mira] Resolved error pattern for tool '{}' (succeeded after {} session failures)",
                    tool_name,
                    session_fp_count
                );
            }
            Ok(())
        })
        .await;
    }

    // Handle Bash commands: detect file-modifying commands and log them
    if post_input.tool_name == "Bash" {
        if let Some(ref command) = post_input.command
            && is_file_modifying_command(command)
        {
            let session_id = post_input.session_id.clone();
            let cmd = crate::utils::truncate(command, 500);
            pool.try_interact("bash file modify logging", move |conn| {
                let mut tracker = BehaviorTracker::for_session(conn, session_id, project_id);
                let data = serde_json::json!({
                    "behavior_type": "bash_file_modify",
                    "command": cmd,
                });
                if let Err(e) = tracker.log_event(conn, crate::proactive::EventType::ToolUse, data)
                {
                    tracing::debug!("Failed to log bash file modify: {e}");
                }
                Ok(())
            })
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
